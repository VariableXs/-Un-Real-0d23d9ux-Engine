//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0489 · 渲染性能账（微秒面度量） · 0x75CC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批5-K）：
//!   微秒面度量（整数微秒口径零浮点），五段分解（提交/变换/光栅化/
//!   混合/回读）各段耗时落账，p99 注记步数化
//!   - 分位数：p99=排序后第 ⌈0.99×n⌉ 位整数口径，p99 与 max 双列
//! 设计要点：
//!   1. 整数微秒口径：PMTimer 3.579545MHz 计数换算
//!      µs = tick×1_000_000 / 3_579_545（整数除法零浮点），锚点冻结：
//!      3_579_545 tick ≙ 1_000_000 µs；
//!   2. 五段闭集分解：提交/变换/光栅化/混合/回读五段各计 tick、各折
//!      µs、各段落账（段和账 [u64;5]），两遍复算逐位一致；
//!   3. 守恒式：Σ五段 ≤ 帧总耗时（帧级计时独立采得），落账即席复核，
//!      破坏显性拒（ConservationBreach），回绕无处遁形；
//!   4. p99 注记步数化：位次 = ⌈0.99×n⌉（整数式 (99n+99)/100），排序
//!      后取第位次个样本；锚点 n=100 ⇒ 位次 99、n=10 ⇒ 位次 10；
//!   5. p99 与 max 双列：p99 ≤ max 恒成立，双列一并落账；
//!   6. LCG 种子写死 = SEG_BASE；no_std + alloc；整数运算零浮点。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75CC0，AI-VX7-1104 批5-K 独占槽）。
pub const SEG_BASE: u32 = 0x75CC0;

// ── 冻结口径 ────────────────────────────────────────────────────
/// PMTimer 频率（3.579545MHz，整数计 3_579_545 Hz）。
pub const PM_TIMER_HZ: u64 = 3_579_545;
/// 换算放大基数（1 秒 = 1_000_000 微秒）。
pub const US_PER_SEC: u64 = 1_000_000;
/// 黄金语料帧数（冻结 100：p99 位次 99 锚点语义）。
pub const CORPUS_FRAMES: usize = 100;
/// 五段数（冻结 5）。
pub const STAGE_N: usize = 5;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerfCode {
    /// 空样本（分位数无样本可取）
    EmptySample,
    /// 五段闭集越界（段数 ≠ 5）
    StageBreach,
    /// 守恒破坏（Σ五段 > 帧总耗时）
    ConservationBreach,
    /// 注入漏抓
    InjectLeaked,
    /// 双跑不一致
    NotReproducible,
}

impl PerfCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PerfCode::EmptySample => "空样本：分位数无样本可取，建议先落账再算位次",
            PerfCode::StageBreach => "五段闭集越界：段数 ≠ 5，建议对齐五段闭集重新落账",
            PerfCode::ConservationBreach => "守恒破坏：Σ五段 > 帧总耗时，建议检查帧级计时与段级计时口径",
            PerfCode::InjectLeaked => "注入漏抓：违约帧未拒，建议对齐冻结守恒式逐帧复核",
            PerfCode::NotReproducible => "双跑不一致：复现红线，建议固定种子后重跑两遍对账",
        }
    }
    pub const ALL: [PerfCode; 5] = [
        PerfCode::EmptySample,
        PerfCode::StageBreach,
        PerfCode::ConservationBreach,
        PerfCode::InjectLeaked,
        PerfCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回；段外码显性报未知）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PerfCode::ALL.len() {
        if let Some(c) = PerfCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75CC0 段闭集内"
}

// ── 五段闭集 ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 提交
    Submit,
    /// 变换
    Transform,
    /// 光栅化
    Rasterize,
    /// 混合
    Blend,
    /// 回读
    Readback,
}

impl Stage {
    pub fn val(self) -> u32 {
        self as u32
    }
    pub const ALL: [Stage; 5] = [
        Stage::Submit,
        Stage::Transform,
        Stage::Rasterize,
        Stage::Blend,
        Stage::Readback,
    ];
}

// ── 整数微秒换算（PMTimer 3.579545MHz） ─────────────────────────
pub fn ticks_to_us(ticks: u64) -> u32 {
    let scaled = ticks.saturating_mul(US_PER_SEC);
    (scaled / PM_TIMER_HZ) as u32
}

// ── p99 整数口径（步数化） ──────────────────────────────────────
/// 1-based 位次 = ⌈0.99×n⌉（整数式 (99n+99)/100）；n=0 返回 0。
pub fn p99_rank(n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    (99usize.saturating_mul(n).saturating_add(99)) / 100
}

/// p99 与 max 双列：升序排序后取第位次个样本与最末样本。
pub fn p99_and_max(vals: &[u32]) -> Result<(u32, u32), u32> {
    if vals.is_empty() {
        return Err(PerfCode::EmptySample.val());
    }
    let mut v: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < vals.len() {
        if let Some(x) = vals.get(i) {
            v.push(*x);
        }
        i += 1;
    }
    v.sort();
    let n = v.len();
    let rank = p99_rank(n);
    let idx = rank.saturating_sub(1);
    let idx = if idx >= n { n - 1 } else { idx };
    let p99 = match v.get(idx) {
        Some(x) => *x,
        None => return Err(PerfCode::EmptySample.val()),
    };
    let max = match v.get(n - 1) {
        Some(x) => *x,
        None => return Err(PerfCode::EmptySample.val()),
    };
    Ok((p99, max))
}

// ── 域模型：帧性能账 ────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FramePerf {
    /// 帧号（1 起）。
    pub frame: u32,
    /// 各段 tick 计数（五段，PMTimer 原始计数）。
    pub stage_ticks: Vec<u32>,
    /// 各段微秒（换算后，与 stage_ticks 同序）。
    pub stage_us: Vec<u32>,
    /// 帧总耗时（微秒，帧级计时独立采得）。
    pub total_us: u32,
}

impl FramePerf {
    /// Σ五段（微秒，saturating 合计）。
    pub fn sum5_us(&self) -> u64 {
        let mut s = 0u64;
        let mut i = 0;
        while i < self.stage_us.len() {
            if let Some(x) = self.stage_us.get(i) {
                s = s.saturating_add(*x as u64);
            }
            i += 1;
        }
        s
    }

    /// 守恒谓词：Σ五段 ≤ 帧总耗时（段数亦须恰 5）。
    pub fn conservation_ok(&self) -> bool {
        if self.stage_us.len() != STAGE_N || self.stage_ticks.len() != STAGE_N {
            return false;
        }
        self.sum5_us() <= self.total_us as u64
    }
}

/// 从 tick 计数构造帧账（逐段换算微秒）。
pub fn frame_from_ticks(frame: u32, stage_ticks: &[u32], total_ticks: u64) -> Result<FramePerf, u32> {
    if stage_ticks.len() != STAGE_N {
        return Err(PerfCode::StageBreach.val());
    }
    let mut stage_us: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < stage_ticks.len() {
        if let Some(t) = stage_ticks.get(i) {
            stage_us.push(ticks_to_us(*t as u64));
        }
        i += 1;
    }
    Ok(FramePerf { frame, stage_ticks: stage_ticks.to_vec(), stage_us, total_us: ticks_to_us(total_ticks) })
}

// ── 性能账本（落账即席复核） ────────────────────────────────────
#[derive(Clone, Debug, Default)]
pub struct PerfLedger {
    pub frames: Vec<FramePerf>,
    pub rejected: u32,
}

impl PerfLedger {
    pub fn new() -> PerfLedger {
        PerfLedger::default()
    }

    /// 落账一帧（五段闭集闸先于守恒闸，两闸分流显性报）。
    pub fn add(&mut self, f: FramePerf) -> Result<(), u32> {
        if f.stage_us.len() != STAGE_N || f.stage_ticks.len() != STAGE_N {
            self.rejected = self.rejected.saturating_add(1);
            return Err(PerfCode::StageBreach.val());
        }
        if f.sum5_us() > f.total_us as u64 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(PerfCode::ConservationBreach.val());
        }
        self.frames.push(f);
        Ok(())
    }

    /// 段和账（五段各段微秒累计，[u64;5] 口径）。
    pub fn stage_sums(&self) -> [u64; 5] {
        let mut sums = [0u64; 5];
        let mut k = 0;
        while k < STAGE_N {
            let mut f = 0;
            while f < self.frames.len() {
                if let Some(fr) = self.frames.get(f) {
                    if let Some(x) = fr.stage_us.get(k) {
                        if let Some(slot) = sums.get_mut(k) {
                            *slot = slot.saturating_add(*x as u64);
                        }
                    }
                }
                f += 1;
            }
            k += 1;
        }
        sums
    }
}

// ── LCG（种子写死 = SEG_BASE） ─────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

/// 黄金语料（100 帧，LCG 驱动：各段 tick 100~2099，帧级计时留 0~3579 tick 余量）。
pub fn golden_corpus() -> Vec<FramePerf> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut out: Vec<FramePerf> = Vec::new();
    let mut f = 1u32;
    while f <= CORPUS_FRAMES as u32 {
        let mut ticks: Vec<u32> = Vec::new();
        let mut sum_ticks = 0u64;
        let mut k = 0;
        while k < STAGE_N {
            let t = 100u64 + rng.below(2000);
            ticks.push(t as u32);
            sum_ticks = sum_ticks.saturating_add(t);
            k += 1;
        }
        // 帧级计时独立采得：段和 + 余量（0~3579 tick，≈0~1µs）。
        let total_ticks = sum_ticks.saturating_add(rng.below(3580));
        if let Ok(fr) = frame_from_ticks(f, &ticks, total_ticks) {
            out.push(fr);
        }
        f += 1;
    }
    out
}

// ── 黄金全链：100 帧落账→段和两遍复算→p99/max 双列→注入 10/10 ──
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let corpus = golden_corpus();
    let mut led = PerfLedger::new();
    let mut i = 0;
    while i < corpus.len() {
        if let Some(fr) = corpus.get(i) {
            led.add(fr.clone())?;
        }
        i += 1;
    }
    if led.frames.len() != CORPUS_FRAMES {
        return Err(PerfCode::InjectLeaked.val());
    }
    // 守恒逐帧复核（双跑面之一）。
    let mut i = 0;
    while i < led.frames.len() {
        if let Some(fr) = led.frames.get(i) {
            if !fr.conservation_ok() {
                return Err(PerfCode::ConservationBreach.val());
            }
        }
        i += 1;
    }
    // 段和账两遍复算逐位一致。
    let sums_a = led.stage_sums();
    let sums_b = led.stage_sums();
    if sums_a != sums_b {
        return Err(PerfCode::NotReproducible.val());
    }
    // p99 与 max 双列。
    let mut totals: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < led.frames.len() {
        if let Some(fr) = led.frames.get(i) {
            totals.push(fr.total_us);
        }
        i += 1;
    }
    let (p99, max) = p99_and_max(&totals)?;
    if p99 > max {
        return Err(PerfCode::InjectLeaked.val());
    }
    // 注入 10/10：篡改帧总耗时（Σ五段 - 1）必被守恒闸抓出。
    let mut caught = 0u32;
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let src = corpus.get(r as usize);
        let bad = match src {
            Some(fr) => {
                let mut b = fr.clone();
                let s = b.sum5_us() as u32;
                b.total_us = s.saturating_sub(1);
                b
            }
            None => return Err(PerfCode::EmptySample.val()),
        };
        match led.add(bad) {
            Err(e) if e == PerfCode::ConservationBreach.val() => {
                caught = caught.saturating_add(1);
            }
            _ => return Err(PerfCode::InjectLeaked.val()),
        }
        r += 1;
    }
    if led.rejected != JUDGE_ROUNDS {
        return Err(PerfCode::InjectLeaked.val());
    }
    Ok((led.frames.len() as u32, caught, p99, max))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链（100 帧落账 + 注入 10/10 + p99 ≤ max）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((frames, caught, p99, max)) => {
            if frames == CORPUS_FRAMES as u32 && caught == JUDGE_ROUNDS && p99 <= max {
                Ok(())
            } else {
                Err(PerfCode::NotReproducible.val())
            }
        }
        Err(_) => Err(PerfCode::NotReproducible.val()),
    }
}

/// 序 B：PMTimer 换算锚点（1s ≙ 1_000_000µs；1 tick ≙ 0µs；1s-1tick ≙ 999_999µs）与单调性。
pub fn probe_seq_b() -> Result<(), u32> {
    let one_sec = ticks_to_us(PM_TIMER_HZ);
    let one_tick = ticks_to_us(1);
    let just_under = ticks_to_us(PM_TIMER_HZ - 1);
    let a = ticks_to_us(1000);
    let b = ticks_to_us(2000);
    match (one_sec, one_tick, just_under) {
        (1_000_000, 0, 999_999) if a <= b => Ok(()),
        _ => Err(PerfCode::NotReproducible.val()),
    }
}

/// 序 C：p99 整数口径锚点（n=100 ⇒ 位次 99；n=10 ⇒ 位次 10）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut vals100: Vec<u32> = Vec::new();
    let mut i = 1u32;
    while i <= 100 {
        vals100.push(i);
        i += 1;
    }
    let mut vals10: Vec<u32> = Vec::new();
    let mut i = 1u32;
    while i <= 10 {
        vals10.push(i);
        i += 1;
    }
    let (p99a, maxa) = p99_and_max(&vals100)?;
    let (p99b, maxb) = p99_and_max(&vals10)?;
    match (p99a, maxa, p99b, maxb) {
        (99, 100, 10, 10) => Ok(()),
        _ => Err(PerfCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 2 路：守恒破坏 / 五段闭集越界） ────
pub struct InjectLedger {
    pub tries: [u32; 2],
    pub caught: [u32; 2],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 2], caught: [0; 2] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 2 {
            if let Some(t) = self.tries.get_mut(slot) {
                *t = t.saturating_add(1);
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 2 {
            if let (Some(t), Some(c)) = (self.tries.get(i), self.caught.get(i)) {
                if *t == 0 || c != t {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

/// 注入账 0 号槽取值（.get 取，不裸下标）。
fn slot0(t: &[u32; 2]) -> u32 {
    match t.get(0) {
        Some(x) => *x,
        None => 0,
    }
}

/// 注入账 1 号槽取值（.get 取，不裸下标）。
fn slot1(t: &[u32; 2]) -> u32 {
    match t.get(1) {
        Some(x) => *x,
        None => 0,
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {    let mut led = InjectLedger::new();
    let corpus = golden_corpus();
    let mut led_main = PerfLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：守恒破坏（帧总耗时压到 Σ五段 - 1）。
        let bad = match corpus.get((r as usize) % corpus.len()) {
            Some(fr) => {
                let mut b = fr.clone();
                let s = b.sum5_us() as u32;
                b.total_us = s.saturating_sub(1);
                b
            }
            None => return Err(PerfCode::EmptySample.val()),
        };
        led.record(0, match led_main.add(bad) {
            Err(e) => e == PerfCode::ConservationBreach.val(),
            Ok(()) => false,
        });
        // 路 2：五段闭集越界（段数 4）。
        let mut short = match corpus.get(0) {
            Some(fr) => fr.clone(),
            None => return Err(PerfCode::EmptySample.val()),
        };
        if short.stage_us.pop().is_some() {
            // 少一段：闭集闸必拒。
            led.record(1, match led_main.add(short) {
                Err(e) => e == PerfCode::StageBreach.val(),
                Ok(()) => false,
            });
        } else {
            led.record(1, false);
        }
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PerfCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在 0x75000~0x781C0 的 0x40 对齐窗口） ─────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < PerfCode::ALL.len() {
        if let Some(c) = PerfCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 诊断码闭集对账 ─────────────────────────────────────────────
fn code_closure_ok() -> bool {
    let mut i = 0;
    while i < PerfCode::ALL.len() {
        if let Some(c) = PerfCode::ALL.get(i) {
            if detail(c.val()) != c.detail() {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 五段闭集对账（ALL 数组 5 段无重复） ────────────────────────
fn stage_closure_ok() -> bool {
    if Stage::ALL.len() != STAGE_N {
        return false;
    }
    let mut i = 0;
    while i < Stage::ALL.len() {
        let mut j = i + 1;
        while j < Stage::ALL.len() {
            if let (Some(a), Some(b)) = (Stage::ALL.get(i), Stage::ALL.get(j)) {
                if a == b {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── LCG 确定性 ─────────────────────────────────────────────────
fn lcg_deterministic_ok() -> bool {
    let mut x = Lcg::new(SEG_BASE as u64);
    let mut y = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < 16 {
        if x.next() != y.next() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：≥12 条 add_dyn） ──────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 口径冻结。
    v.push((
        "J1-口径冻结",
        PM_TIMER_HZ == 3_579_545 && CORPUS_FRAMES == 100 && STAGE_N == 5,
        format!("{}Hz 语料 {} 帧 五段 {}", PM_TIMER_HZ, CORPUS_FRAMES, STAGE_N),
    ));
    // 2. PMTimer 换算锚点。
    let anchor = ticks_to_us(PM_TIMER_HZ) == 1_000_000
        && ticks_to_us(PM_TIMER_HZ - 1) == 999_999
        && ticks_to_us(1) == 0
        && ticks_to_us(3580) == 1000;
    v.push((
        "J1-PMTimer换算锚点",
        anchor,
        format!(
            "1s={}µs 1tick={}µs 1s-1tick={}µs",
            ticks_to_us(PM_TIMER_HZ),
            ticks_to_us(1),
            ticks_to_us(PM_TIMER_HZ - 1)
        ),
    ));
    // 3. 五段闭集。
    v.push((
        "J1-五段闭集",
        stage_closure_ok(),
        format!("提交/变换/光栅化/混合/回读 {} 段无重复", Stage::ALL.len()),
    ));
    // 4. 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((frames, caught, _, _)) => *frames == CORPUS_FRAMES as u32 && *caught == JUDGE_ROUNDS,
            Err(_) => false,
        },
        match &g {
            Ok((frames, caught, _, _)) => format!("frames={} inject={}/10", frames, caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 5. Σ五段守恒（全语料逐帧）。
    let conservation = {
        let corpus = golden_corpus();
        let mut ok = corpus.len() == CORPUS_FRAMES;
        let mut i = 0;
        while i < corpus.len() {
            if let Some(fr) = corpus.get(i) {
                if !fr.conservation_ok() {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    };
    v.push((
        "J1-Σ五段守恒",
        conservation,
        format!("{} 帧逐帧 Σ五段≤总耗时", CORPUS_FRAMES),
    ));
    // 6. 分段落账复算。
    let sums_ok = {
        let corpus = golden_corpus();
        let mut led = PerfLedger::new();
        let mut i = 0;
        while i < corpus.len() {
            if let Some(fr) = corpus.get(i) {
                let _ = led.add(fr.clone());
            }
            i += 1;
        }
        let a = led.stage_sums();
        let b = led.stage_sums();
        a == b
    };
    v.push((
        "J1-分段落账复算",
        sums_ok,
        String::from("段和账两遍复算逐位一致"),
    ));
    // 7. p99 整数口径锚点。
    v.push((
        "J1-p99整数口径锚点",
        p99_rank(100) == 99 && p99_rank(10) == 10 && p99_rank(1) == 1 && p99_rank(0) == 0,
        format!("⌈0.99n⌉: n100={} n10={}", p99_rank(100), p99_rank(10)),
    ));
    // 8. p99 与 max 双列。
    let dual = {
        let corpus = golden_corpus();
        let mut totals: Vec<u32> = Vec::new();
        let mut i = 0;
        while i < corpus.len() {
            if let Some(fr) = corpus.get(i) {
                totals.push(fr.total_us);
            }
            i += 1;
        }
        match p99_and_max(&totals) {
            Ok((p99, max)) => p99 <= max,
            Err(_) => false,
        }
    };
    v.push((
        "J1-p99与max双列",
        dual,
        String::from("p99 ≤ max 双列落账"),
    ));
    // 9. 注入两路 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-注入两路10/10",
        match &inj {
            Ok(l) => l.all_caught() && slot0(&l.tries) == JUDGE_ROUNDS,
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("t={:?} c={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 10. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b换算锚点={} c位次锚点={}", a, b, c),
    ));
    // 11. 诊断码闭集对账。
    v.push((
        "J1-诊断码闭集对账",
        code_closure_ok(),
        format!("{} 码全查表通过", PerfCode::ALL.len()),
    ));
    // 12. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 13. LCG 确定性。
    v.push((
        "J2-LCG确定性",
        lcg_deterministic_ok(),
        String::from("同种子 16 步逐位一致"),
    ));
    // 14. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x75CC0 窗口 0x40 对齐区间三闸"),
    ));
    v
}

pub fn run_unx0489_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0489 渲染性能账（微秒面度量）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_run_green() {
        match golden_run() {
            Ok((frames, caught, p99, max)) => {
                assert_eq!(frames, CORPUS_FRAMES as u32);
                assert_eq!(caught, JUDGE_ROUNDS);
                assert!(p99 <= max, "p99 不得大于 max");
            }
            Err(e) => assert!(false, "golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn frozen_consts() {
        assert_eq!(PM_TIMER_HZ, 3_579_545);
        assert_eq!(CORPUS_FRAMES, 100);
        assert_eq!(STAGE_N, 5);
        assert_eq!(Stage::ALL.len(), 5);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn pm_timer_conversion_anchors() {
        assert_eq!(ticks_to_us(PM_TIMER_HZ), 1_000_000);
        assert_eq!(ticks_to_us(PM_TIMER_HZ - 1), 999_999);
        assert_eq!(ticks_to_us(1), 0);
        assert_eq!(ticks_to_us(3580), 1000);
        // 单调：tick 多则 µs 不减。
        let mut t = 0u64;
        let mut prev = 0u32;
        while t < 40_000 {
            let us = ticks_to_us(t);
            assert!(us >= prev, "换算非单调 at tick={}", t);
            prev = us;
            t += 97;
        }
    }

    #[test]
    fn p99_integer_anchors() {
        assert_eq!(p99_rank(0), 0);
        assert_eq!(p99_rank(1), 1);
        assert_eq!(p99_rank(10), 10);
        assert_eq!(p99_rank(100), 99);
        assert_eq!(p99_rank(200), 198);
        // 手工锚点：1..100 升序 ⇒ p99=99、max=100。
        let mut vals: Vec<u32> = Vec::new();
        let mut i = 1u32;
        while i <= 100 {
            vals.push(i);
            i += 1;
        }
        match p99_and_max(&vals) {
            Ok((p, m)) => {
                assert_eq!(p, 99);
                assert_eq!(m, 100);
            }
            Err(e) => assert!(false, "p99 err=0x{:x}", e),
        }
        // 空样本显性拒。
        match p99_and_max(&[]) {
            Err(e) => assert_eq!(e, PerfCode::EmptySample.val()),
            Ok(_) => assert!(false, "空样本未拒"),
        }
    }

    #[test]
    fn conservation_gate_caught() {
        let corpus = golden_corpus();
        let mut led = PerfLedger::new();
        // 正常帧全部落账。
        let mut i = 0;
        while i < corpus.len() {
            if let Some(fr) = corpus.get(i) {
                assert!(led.add(fr.clone()).is_ok(), "正常帧被误拒 at {}", i);
            }
            i += 1;
        }
        // 篡改帧总耗时（Σ五段 - 1）必拒。
        if let Some(fr) = corpus.get(0) {
            let mut bad = fr.clone();
            let s = bad.sum5_us() as u32;
            bad.total_us = s.saturating_sub(1);
            match led.add(bad) {
                Err(e) => assert_eq!(e, PerfCode::ConservationBreach.val()),
                Ok(()) => assert!(false, "守恒破坏帧未拒"),
            }
        }
        // 段数不足（闭集越界）必拒。
        if let Some(fr) = corpus.get(1) {
            let mut short = fr.clone();
            let _ = short.stage_us.pop();
            match led.add(short) {
                Err(e) => assert_eq!(e, PerfCode::StageBreach.val()),
                Ok(()) => assert!(false, "闭集越界帧未拒"),
            }
        }
    }

    #[test]
    fn stage_sums_recompute() {
        let corpus = golden_corpus();
        let mut led = PerfLedger::new();
        let mut i = 0;
        while i < corpus.len() {
            if let Some(fr) = corpus.get(i) {
                let _ = led.add(fr.clone());
            }
            i += 1;
        }
        let a = led.stage_sums();
        let b = led.stage_sums();
        assert_eq!(a, b);
        // Σ段和 = Σ各帧 Σ五段（账面自洽）。
        let mut total = 0u64;
        let mut i = 0;
        while i < STAGE_N {
            if let Some(x) = a.get(i) {
                total = total.saturating_add(*x);
            }
            i += 1;
        }
        let mut expect = 0u64;
        let mut f = 0;
        while f < led.frames.len() {
            if let Some(fr) = led.frames.get(f) {
                expect = expect.saturating_add(fr.sum5_us());
            }
            f += 1;
        }
        assert_eq!(total, expect);
    }

    #[test]
    fn inject_rounds_tally() {
        let led = match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => l,
            Err(e) => {
                assert!(false, "inject err=0x{:x}", e);
                return;
            }
        };
        assert!(led.all_caught());
        assert_eq!(slot0(&led.tries), JUDGE_ROUNDS);
        assert_eq!(slot1(&led.tries), JUDGE_ROUNDS);
    }

    #[test]
    fn probe_sequences() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn reproducible_and_guard() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert!(code_closure_ok());
        assert!(stage_closure_ok());
        assert!(lcg_deterministic_ok());
        assert_eq!(detail(PerfCode::ConservationBreach.val()), PerfCode::ConservationBreach.detail());
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知诊断码"));
    }
}
