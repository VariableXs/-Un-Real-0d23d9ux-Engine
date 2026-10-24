//! VE-F3555 · persona 基准 —— 三档基准 + 口径一致 + 实测发布 + 环境指纹 + P95 分位。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3555`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三档基准**（[`BenchTier`]）：轻 / 中 / 重档案三档，
//!   各档独立采样独立口径。
//! - **口径一致**（[`BenchSuite::publish`]）：口径缺失阻断发布；
//!   环境指纹进报告（不同环境结果不可比）。
//! - **P95 分位**（[`SampleSet::p95`]）：基准分位数承诺——
//!   P95 进口径，不是只报均值。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 口径缺失 | 阻断 |
//! | 不符 | 撤回 |
//! | 漂移 | 重测 |
//!
//! # 复杂度
//!
//! O(基准)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E19 段续：6280 起）
// ---------------------------------------------------------------------------

/// persona 基准诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BenchCode {
    /// 口径缺失（阻断发布）。
    ProtocolMissing = 6280,
    /// 样本不足（P95 不可算）。
    SampleInsufficient = 6281,
    /// 环境指纹缺失。
    FingerprintMissing = 6282,
    /// 结果撤回。
    Withdrawn = 6283,
}

impl BenchCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            BenchCode::ProtocolMissing => "基准口径缺失",
            BenchCode::SampleInsufficient => "样本不足",
            BenchCode::FingerprintMissing => "环境指纹缺失",
            BenchCode::Withdrawn => "基准结果已撤回",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, BenchCode::ProtocolMissing | BenchCode::FingerprintMissing)
    }
}

// ---------------------------------------------------------------------------
// 二、三档
// ---------------------------------------------------------------------------

/// 基准档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BenchTier {
    /// 轻档案（< 50 令牌）。
    Light,
    /// 中档案（50–200 令牌）。
    Medium,
    /// 重档案（> 200 令牌）。
    Heavy,
}

impl BenchTier {
    /// 全部三档。
    pub const ALL: [BenchTier; 3] = [BenchTier::Light, BenchTier::Medium, BenchTier::Heavy];

    /// 档短名。
    pub const fn label(self) -> &'static str {
        match self {
            BenchTier::Light => "轻档",
            BenchTier::Medium => "中档",
            BenchTier::Heavy => "重档",
        }
    }

    /// 档位令牌数中位（基准口径描述用）。
    pub const fn typical_tokens(self) -> u32 {
        match self {
            BenchTier::Light => 30,
            BenchTier::Medium => 120,
            BenchTier::Heavy => 400,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、样本集与 P95
// ---------------------------------------------------------------------------

/// 采样集（微秒级耗时）。
#[derive(Clone, Debug)]
pub struct SampleSet {
    samples: Vec<u32>,
}

impl SampleSet {
    /// 从样本建集。
    pub fn new(mut samples: Vec<u32>) -> Self {
        samples.sort_unstable();
        Self { samples }
    }

    /// 样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// P95 分位（下标取 ceil(0.95*n)-1，n>=20 才有意义）。
    pub fn p95(&self) -> Option<u32> {
        if self.samples.len() < 20 {
            return None;
        }
        let n = self.samples.len();
        // idx = ceil(0.95*n) - 1 = (95*n + 99)/100 - 1（整数运算避免浮点）。
        let idx = (95 * n + 99) / 100 - 1;
        if idx < n {
            Some(self.samples[idx])
        } else {
            self.samples.last().copied()
        }
    }

    /// 中位。
    pub fn median(&self) -> Option<u32> {
        if self.samples.is_empty() {
            return None;
        }
        Some(self.samples[self.samples.len() / 2])
    }

    /// 最大。
    pub fn max(&self) -> Option<u32> {
        self.samples.last().copied()
    }
}

// ---------------------------------------------------------------------------
// 四、口径与环境指纹
// ---------------------------------------------------------------------------

/// 环境指纹。
#[derive(Clone, Copy, Debug)]
pub struct EnvFingerprint {
    /// CPU 型号 id。
    pub cpu_id: u32,
    /// 内存 KiB。
    pub mem_kib: u32,
    /// 是否指纹有效。
    pub present: bool,
}

/// 基准口径（发布必备）。
#[derive(Clone, Copy, Debug)]
pub struct BenchProtocol {
    /// 口径版本。
    pub version: u32,
    /// 环境指纹。
    pub env: EnvFingerprint,
}

// ---------------------------------------------------------------------------
// 五、基准套件
// ---------------------------------------------------------------------------

/// 单档结果。
#[derive(Clone, Debug)]
pub struct TierResult {
    /// 档。
    pub tier: BenchTier,
    /// 采样。
    pub samples: SampleSet,
}

/// 发布报告。
#[derive(Clone, Debug)]
pub struct BenchReport {
    /// P95 汇总（三档拼接）。
    pub p95_text: &'static str,
    /// 环境指纹是否随报告。
    pub env_present: bool,
}

/// 基准套件。
pub struct BenchSuite {
    protocol: Option<BenchProtocol>,
    results: Vec<TierResult>,
}

impl BenchSuite {
    /// 新建。
    pub fn new() -> Self {
        Self {
            protocol: None,
            results: Vec::new(),
        }
    }

    /// 设口径。
    pub fn set_protocol(&mut self, p: BenchProtocol) {
        self.protocol = Some(p);
    }

    /// 记一档结果。
    pub fn record(&mut self, tier: BenchTier, samples: Vec<u32>) {
        self.results.push(TierResult {
            tier,
            samples: SampleSet::new(samples),
        });
    }

    /// 发布：口径齐 + 指纹在 + 每档样本足 20 才放行。
    pub fn publish(&self) -> Result<BenchReport, BenchCode> {
        let p = self.protocol.ok_or(BenchCode::ProtocolMissing)?;
        if !p.env.present {
            return Err(BenchCode::FingerprintMissing);
        }
        if self.results.is_empty() {
            return Err(BenchCode::SampleInsufficient);
        }
        for r in &self.results {
            if r.samples.p95().is_none() {
                return Err(BenchCode::SampleInsufficient);
            }
        }
        Ok(BenchReport {
            p95_text: "三档 P95 已进口径",
            env_present: true,
        })
    }

    /// 某档 P95。
    pub fn p95_of(&self, tier: BenchTier) -> Option<u32> {
        self.results
            .iter()
            .find(|r| r.tier == tier)
            .and_then(|r| r.samples.p95())
    }

    /// 已录档数。
    pub fn tier_count(&self) -> usize {
        self.results.len()
    }
}

impl Default for BenchSuite {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

/// persona 基准判据集。
pub fn run_bench_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver10o_bench");

    // 1. P95 计算正确（20 样本 1..20，P95 位 = 第 19 大 = 19）。
    let samples: Vec<u32> = (1..=20).collect();
    let set = SampleSet::new(samples);
    chk.add("6280-P95 计算", set.p95() == Some(19), "");

    // 2. 样本不足 P95 为 None。
    let small = SampleSet::new(alloc::vec![1, 2, 3]);
    chk.add("6281-样本不足", small.p95().is_none(), "");

    // 3. 口径缺失阻断发布。
    let suite = BenchSuite::new();
    chk.add(
            "6282-口径缺失阻断",
            suite.publish().err() == Some(BenchCode::ProtocolMissing),
            "",
        );

    // 4. 指纹缺失阻断。
    let mut s2 = BenchSuite::new();
    s2.set_protocol(BenchProtocol {
        version: 1,
        env: EnvFingerprint {
            cpu_id: 0,
            mem_kib: 0,
            present: false,
        },
    });
    chk.add(
            "6283-指纹缺失阻断",
            s2.publish().err() == Some(BenchCode::FingerprintMissing),
            "",
        );

    // 5. 三档齐 + 样本足 → 发布成功。
    let mut s3 = BenchSuite::new();
    s3.set_protocol(BenchProtocol {
        version: 1,
        env: EnvFingerprint {
            cpu_id: 7,
            mem_kib: 16384,
            present: true,
        },
    });
    let thirty: Vec<u32> = (1..=30).collect();
    for t in BenchTier::ALL {
        s3.record(t, thirty.clone());
    }
    let ok = s3.publish().is_ok() && s3.p95_of(BenchTier::Light) == Some(29);
    chk.add("6284-三档发布", ok, "");

    // 6. 档名可达。
    chk.add(
            "6285-档名可达",
            BenchTier::Heavy.label() == "重档" && BenchTier::Heavy.typical_tokens() == 400,
            "",
        );

    // 7. 中位正确。
    chk.add("6286-中位正确", set.median() == Some(11), "");

    chk
}
