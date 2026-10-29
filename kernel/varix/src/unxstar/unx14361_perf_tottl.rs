//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14361 · 性能核账总起（四面账总框架） · 0x66600
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F14361-J1 性能总框架：域性能四面账总图（开键/枚举/查询/写
//!      入四面×口径×水位×闸位四列 4 行总表），四面口径与既有判据
//!      （F14217/F14235 口径族）一致断言，水位推导表显式断言（样本分
//!      位数），与 B37 资源面分界表，指纹钉版，红绿齐。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 功能定位：性能总框架是域性能收口的总地图——四面口径一致+水位推导
//!   显式是性能结论可信的第一证明。
//! 语义边界：只做四面账总框架；不做单面实测（后续条持有）、不做资源
//!   上限（B37 持有）、不做坏档性能（B38 持有）。
//! 设计要点：
//!   - 四面闭集：开键 0/枚举 1/查询 2/写入 3；
//!   - 口径闭集钉版：开键/查询归 F14217 口径族（微秒每键）、枚举/写入
//!     归 F14235 口径族（微秒每项）——口径列写死不得漂；
//!   - 水位推导：样本分位数整数最近邻法（禁浮点）：p50/p90/p99 三档，
//!     序号 = (n*p + 99)/100 上取整，取升序样本第 k 位；
//!   - 推导对拍：直排法（排序取位）× 桶谱法（256 桶计数重建）双路
//!     推导必须一致（显式断言）；
//!   - B37 资源面分界：资源上限口径不入四面账（分界闸钉 0），注入必红；
//!   - 闸位闭集：0=未挂/1=已挂/2=过闸/3=封版；
//!   - LCG 种子写死 = 0x66600；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66600，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66600;

/// 四面闭集。
pub const FACE_OPENKEY: usize = 0;
pub const FACE_ENUM: usize = 1;
pub const FACE_QUERY: usize = 2;
pub const FACE_WRITE: usize = 3;
pub const FACE_COUNT: usize = 4;
pub const FACE_NAMES: [&str; FACE_COUNT] = ["开键面", "枚举面", "查询面", "写入面"];

/// 口径族闭集（来源：F14217/F14235 口径族，写死）。
pub const CAL_F14217: usize = 0;
pub const CAL_F14235: usize = 1;
pub const CALIBER_COUNT: usize = 2;
pub const CALIBER_NAMES: [&str; CALIBER_COUNT] =
    ["F14217 微秒每键", "F14235 微秒每项"];

/// 每面样本数（写死：分位数推导样本 64 点，来源：水位推导表口径）。
pub const SAMPLES_PER_FACE: usize = 64;
/// 桶谱重建桶数（256 桶覆盖 0..4095 计量区间）。
pub const BUCKETS: usize = 256;
pub const BUCKET_SPAN: usize = 16;
/// 计量上限（写死：单样本 0..4095 微秒，越界样本拒收）。
pub const SAMPLE_MAX: usize = 4095;

/// 闸位闭集。
pub const GATE_OPEN: u8 = 0;
pub const GATE_HUNG: u8 = 1;
pub const GATE_PASSED: u8 = 2;
pub const GATE_SEALED: u8 = 3;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerfCode {
    /// 面越闭集（0..4）
    FaceOutOfRange,
    /// 口径越闭集（F14217/F14235 两族）
    CaliberInvalid,
    /// 样本越计量区间（>4095）
    SampleOutOfRange,
    /// 样本账空（分位数无推导基础）
    SamplesEmpty,
    /// 分位序号越界
    PercentileIndexInvalid,
    /// 双路推导不一致（直排×桶谱）
    DerivationMismatch,
    /// 口径与既有判据不一致
    CaliberDrift,
    /// B37 资源面口径混入（分界红线）
    ResourceMixed,
    /// 闸位越闭集
    GateOutOfRange,
    /// 总表行数 ≠ 4
    TableRowMismatch,
    /// 双机对拍漂移
    DualReplayDrift,
}

impl PerfCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PerfCode::FaceOutOfRange => "面越闭集：开键/枚举/查询/写入四面",
            PerfCode::CaliberInvalid => "口径越闭集：F14217/F14235 两族",
            PerfCode::SampleOutOfRange => "样本越计量区间：0..4095 微秒",
            PerfCode::SamplesEmpty => "样本账空：水位无推导基础",
            PerfCode::PercentileIndexInvalid => "分位序号越界：最近邻法闭式",
            PerfCode::DerivationMismatch => "双路推导不一致：直排×桶谱",
            PerfCode::CaliberDrift => "口径漂移：与 F14217/F14235 不一致",
            PerfCode::ResourceMixed => "B37 资源面口径混入：分界红线",
            PerfCode::GateOutOfRange => "闸位越闭集：0..3",
            PerfCode::TableRowMismatch => "总表行数 ≠ 4：四面账红线",
            PerfCode::DualReplayDrift => "双机对拍漂移：水位不可复现",
        }
    }
    pub const ALL: [PerfCode; 11] = [
        PerfCode::FaceOutOfRange,
        PerfCode::CaliberInvalid,
        PerfCode::SampleOutOfRange,
        PerfCode::SamplesEmpty,
        PerfCode::PercentileIndexInvalid,
        PerfCode::DerivationMismatch,
        PerfCode::CaliberDrift,
        PerfCode::ResourceMixed,
        PerfCode::GateOutOfRange,
        PerfCode::TableRowMismatch,
        PerfCode::DualReplayDrift,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PerfCode::ALL.len() {
        if PerfCode::ALL[i].val() == code {
            return PerfCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66600 段闭集内"
}

/// 四面口径钉版（写死：开键 0/枚举 1/查询 0/写入 1，来源：F14217
/// 承开键+查询、F14235 承枚举+写入）。
pub fn pinned_caliber(face: usize) -> Result<usize, u32> {
    if face >= FACE_COUNT {
        return Err(PerfCode::FaceOutOfRange.val());
    }
    match face {
        FACE_OPENKEY => Ok(CAL_F14217),
        FACE_ENUM => Ok(CAL_F14235),
        FACE_QUERY => Ok(CAL_F14217),
        _ => Ok(CAL_F14235),
    }
}

// ── 分位数整数最近邻推导 ─────────────────────────────────────────
/// 直排法：升序样本取第 k 位，k = (n*p + 99)/100（上取整，p∈1..100）。
pub fn percentile_rank(n: usize, p: usize) -> Result<usize, u32> {
    if n == 0 {
        return Err(PerfCode::SamplesEmpty.val());
    }
    if p == 0 || p > 100 {
        return Err(PerfCode::PercentileIndexInvalid.val());
    }
    let k = (n * p + 99) / 100;
    if k == 0 || k > n {
        return Err(PerfCode::PercentileIndexInvalid.val());
    }
    Ok(k)
}

/// 直排法取值（升序样本 + 分位档 p）。
pub fn percentile_direct(sorted: &[usize], p: usize) -> Result<usize, u32> {
    if sorted.is_empty() {
        return Err(PerfCode::SamplesEmpty.val());
    }
    let k = percentile_rank(sorted.len(), p)?;
    if k > sorted.len() {
        return Err(PerfCode::PercentileIndexInvalid.val());
    }
    Ok(sorted[k - 1])
}

/// 桶谱法取值：256 桶计数重建（升序等价：按桶累计计数跨过 k 的桶取
/// 桶中位值）。
pub fn percentile_buckets(hist: &[usize; BUCKETS], n: usize, p: usize) -> Result<usize, u32> {
    if hist.len() != BUCKETS {
        return Err(PerfCode::DerivationMismatch.val());
    }
    if n == 0 {
        return Err(PerfCode::SamplesEmpty.val());
    }
    let k = percentile_rank(n, p)?;
    let mut acc = 0;
    let mut b = 0;
    while b < BUCKETS {
        acc += hist[b];
        if acc >= k {
            // 桶中位值：桶首值 + 桶宽/2 - 1（整数闭式）。
            let base = b * BUCKET_SPAN;
            let mid = BUCKET_SPAN / 2;
            if mid == 0 {
                return Err(PerfCode::DerivationMismatch.val());
            }
            return Ok(base + mid - 1);
        }
        b += 1;
    }
    Err(PerfCode::PercentileIndexInvalid.val())
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

/// 四面样本构造（确定性：面种子 = SEG_BASE + 面 × 0x10）。
pub fn build_samples(face: usize) -> Result<Vec<usize>, u32> {
    if face >= FACE_COUNT {
        return Err(PerfCode::FaceOutOfRange.val());
    }
    let mut rng = Lcg::new((SEG_BASE + (face as u32) * 0x10) as u64);
    let mut v: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < SAMPLES_PER_FACE {
        let s = (rng.below((SAMPLE_MAX + 1) as u64)) as usize;
        if s > SAMPLE_MAX {
            return Err(PerfCode::SampleOutOfRange.val());
        }
        v.push(s);
        i += 1;
    }
    Ok(v)
}

/// 升序副本（插入排序，样本量 64 闭式可控）。
pub fn sorted_copy(src: &[usize]) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < src.len() {
        out.push(src[i]);
        i += 1;
    }
    let mut i = 1;
    while i < out.len() {
        let mut j = i;
        while j > 0 && out[j] < out[j - 1] {
            let t = out[j];
            out[j] = out[j - 1];
            out[j - 1] = t;
            j -= 1;
        }
        i += 1;
    }
    out
}

/// 桶谱直方（样本 → 256 桶计数）。
pub fn histogram(samples: &[usize]) -> Result<[usize; BUCKETS], u32> {
    let mut h = [0usize; BUCKETS];
    let mut i = 0;
    while i < samples.len() {
        if samples[i] > SAMPLE_MAX {
            return Err(PerfCode::SampleOutOfRange.val());
        }
        let b = samples[i] / BUCKET_SPAN;
        if b >= BUCKETS {
            return Err(PerfCode::SampleOutOfRange.val());
        }
        h[b] += 1;
        i += 1;
    }
    Ok(h)
}

// ── 四面账总表行（面/口径/水位/闸位） ───────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceRow {
    pub face: usize,
    pub caliber: usize,
    /// 水位三档（p50/p90/p99，直排法推导）
    pub water: [usize; 3],
    pub gate: u8,
}

pub struct PerfLedger {
    pub rows: Vec<FaceRow>,
    pub sealed: bool,
}

impl PerfLedger {
    pub fn new() -> PerfLedger {
        PerfLedger { rows: Vec::new(), sealed: false }
    }

    /// 四面账总表落位（4 行，水位双路推导并显式断言）。
    pub fn build(&mut self) -> Result<(), u32> {
        if !self.rows.is_empty() {
            return Err(PerfCode::TableRowMismatch.val());
        }
        let mut f = 0;
        while f < FACE_COUNT {
            let caliber = pinned_caliber(f)?;
            let samples = build_samples(f)?;
            if samples.len() != SAMPLES_PER_FACE {
                return Err(PerfCode::SamplesEmpty.val());
            }
            let sorted = sorted_copy(&samples);
            let hist = histogram(&samples)?;
            // 三档水位双路推导对拍（显式断言）。
            let mut water = [0usize; 3];
            let ps: [usize; 3] = [50, 90, 99];
            let mut k = 0;
            while k < 3 {
                let d = percentile_direct(&sorted, ps[k])?;
                let b = percentile_buckets(&hist, samples.len(), ps[k])?;
                // 桶谱法粒度=桶中位，与直排法逐点一致性以同桶为判：
                // 直排值与桶谱值必须落同桶（差 < BUCKET_SPAN）。
                if d / BUCKET_SPAN != b / BUCKET_SPAN {
                    return Err(PerfCode::DerivationMismatch.val());
                }
                water[k] = d;
                k += 1;
            }
            self.rows.push(FaceRow { face: f, caliber, water, gate: GATE_HUNG });
            f += 1;
        }
        if self.rows.len() != FACE_COUNT {
            return Err(PerfCode::TableRowMismatch.val());
        }
        Ok(())
    }

    /// 口径一致断言：逐面与既有判据口径族对表。
    pub fn caliber_consistent(&self) -> Result<(), u32> {
        if self.rows.len() != FACE_COUNT {
            return Err(PerfCode::TableRowMismatch.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            // 独立写死判据常量（不复用 pinned_caliber 的返回值）：
            // 开键=0、枚举=1、查询=0、写入=1。
            let expect = match i {
                0 => CAL_F14217,
                1 => CAL_F14235,
                2 => CAL_F14217,
                _ => CAL_F14235,
            };
            if self.rows[i].caliber != expect {
                return Err(PerfCode::CaliberDrift.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// B37 资源面分界：四面账中不得出现资源上限口径（钉 0x7F 值注入
    /// 检测——任何行口径 ≥ CALIBER_COUNT 即红线）。
    pub fn resource_boundary(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].caliber >= CALIBER_COUNT {
                return Err(PerfCode::ResourceMixed.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 过闸→封版。
    pub fn pass_all(&mut self) -> Result<usize, u32> {
        if self.rows.len() != FACE_COUNT {
            return Err(PerfCode::TableRowMismatch.val());
        }
        let mut passed = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].gate != GATE_HUNG && self.rows[i].gate != GATE_PASSED {
                return Err(PerfCode::GateOutOfRange.val());
            }
            self.rows[i].gate = GATE_PASSED;
            passed += 1;
            i += 1;
        }
        let mut i = 0;
        while i < self.rows.len() {
            self.rows[i].gate = GATE_SEALED;
            i += 1;
        }
        self.sealed = true;
        Ok(passed)
    }

    /// 双机对拍：第二账本独立落位，逐行一致。
    pub fn dual_replay(&self) -> Result<(), u32> {
        let mut twin = PerfLedger::new();
        twin.build()?;
        if twin.rows.len() != self.rows.len() {
            return Err(PerfCode::DualReplayDrift.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if twin.rows[i] != self.rows[i] {
                return Err(PerfCode::DualReplayDrift.val());
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(usize, [usize; 3]), u32> {
    let mut led = PerfLedger::new();
    led.build()?;
    led.caliber_consistent()?;
    led.resource_boundary()?;
    led.dual_replay()?;
    let water = led.rows[0].water;
    let passed = led.pass_all()?;
    if passed != FACE_COUNT {
        return Err(PerfCode::TableRowMismatch.val());
    }
    Ok((led.rows.len(), water))
}

// ── 注入面：三族 3/3 必抓 ────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：B37 资源面口径混入（口径 0x7F 注入）。
    ledger.tries[0] += 1;
    let mut led = PerfLedger::new();
    let _ = led.build();
    if led.rows.len() > 1 {
        led.rows[1].caliber = 0x7F;
    }
    if led.resource_boundary().is_err() {
        ledger.caught[0] += 1;
    }
    // 族 1：口径漂移注入（查询面口径改挂 F14235）。
    ledger.tries[1] += 1;
    let mut led2 = PerfLedger::new();
    let _ = led2.build();
    if led2.rows.len() > 2 {
        led2.rows[2].caliber = CAL_F14235;
    }
    if led2.caliber_consistent().is_err() {
        ledger.caught[1] += 1;
    }
    // 族 2：越界样本注入（4096 计量拒收 + 空样本拒收）。
    ledger.tries[2] += 2;
    let over = histogram(&[SAMPLE_MAX + 1]);
    if over.is_err() {
        ledger.caught[2] += 1;
    }
    let empty: Vec<usize> = Vec::new();
    let sorted = sorted_copy(&empty);
    if percentile_direct(&sorted, 50).is_err() {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PerfCode::ALL.len() {
        let v = PerfCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：4 行四面账 + 过闸封版 + 水位非零。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((rows, water)) => {
            *rows == FACE_COUNT && water[0] > 0 && water[1] >= water[0] && water[2] >= water[1]
        }
        Err(_) => false,
    };
    v.push(("J1-4行四面账+水位单调", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. 口径一致独立复核。
    let caliber_ok = {
        let mut led = PerfLedger::new();
        match led.build() {
            Ok(()) => led.caliber_consistent().is_ok(),
            Err(_) => false,
        }
    };
    v.push((
        "J1-F14217/F14235 口径一致",
        caliber_ok,
        String::from("四面对表"),
    ));
    // 4. 水位推导表显式断言（64 样本三档双路对拍）。
    let derive_ok = {
        let samples = build_samples(FACE_QUERY);
        match samples {
            Ok(s) => {
                let sorted = sorted_copy(&s);
                let hist = histogram(&s);
                match hist {
                    Ok(h) => {
                        let mut ok = true;
                        let ps: [usize; 3] = [50, 90, 99];
                        let mut k = 0;
                        while k < 3 {
                            let d = percentile_direct(&sorted, ps[k]);
                            let b = percentile_buckets(&h, s.len(), ps[k]);
                            match (d, b) {
                                (Ok(dv), Ok(bv)) => {
                                    if dv / BUCKET_SPAN != bv / BUCKET_SPAN {
                                        ok = false;
                                    }
                                }
                                _ => ok = false,
                            }
                            k += 1;
                        }
                        ok
                    }
                    Err(_) => false,
                }
            }
            Err(_) => false,
        }
    };
    v.push((
        "J1-水位推导显式（直排×桶谱）",
        derive_ok,
        String::from("p50/p90/p99"),
    ));
    // 5. 分位序号闭式抽验：n=64, p=50 → k=32。
    v.push((
        "J1-分位序号闭式抽验",
        percentile_rank(64, 50) == Ok(32),
        String::from("n=64 p=50 k=32"),
    ));
    // 6. 诊断码窗口守卫。
    v.push(("J3-段位守卫", segment_guard(), format!("{:#x} 窗口", SEG_BASE)));
    // 7. LCG 确定性。
    let lcg_ok = {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        let mut ok = true;
        let mut i = 0;
        while i < 32 {
            if a.next() != b.next() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-LCG 确定性对拍", lcg_ok, format!("seed={:#x}", SEG_BASE)));
    v
}

pub fn run_unx14361_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14361 性能核账总起（四面账总框架）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
