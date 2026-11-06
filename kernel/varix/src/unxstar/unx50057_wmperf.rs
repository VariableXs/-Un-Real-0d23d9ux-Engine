//! UNX-F50057 水印性能预算（合成耗时上限模拟器档）（0x43340 段）
//!
//! 锚点：UNX-F50057-J1 实测 ≤预算登记；判据锚定 §AI-63 §5。
//!
//! 数据段：`SEG_BASE = 0x43340`；诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 8`，
//! 逐项见 [`PerfError`]。
//!
//! 为什么要有「预算」而不是「尽量快」：水印合成挂在打印主路径上，无上限的
//! 实现只能事后抱怨。本模块把「每页合成耗时上限」按 dpi 档显式登记
//! （[`TIERS`]），实测值由**纯整数成本模型**算出——同一个 job 两次测量逐位
//! 一致，判据才敢拿「实测 ≤ 登记」当硬门，而不是拿墙钟时间赌 CI 负载。
//! 时间单位 tick 定义为 1 µs（模拟器档标定，见 [`RASTER_UNITS_PER_KPX`]）。
//!
//! 结构清单：
//! * [`Grade`]：模拟器精度三档（草样 / 标准 / 打样），倍率写成整数分数；
//! * [`Job`] + [`measure`]：单页成本模型（光栅 / 字形 / 半透明合成三项相加）；
//! * [`Stage`] + [`Meter`]：单页测量状态机——未测量不许登记；
//! * [`PerfLedger`]：预算登记账（入账即比上限；重定价后全表重算抓漂移）；
//! * [`injection_core`]：7 类坏样本注入核（零尺寸 / 未知档位 / 越预算 /
//!   未测量 / 重号 / 容量满 / 精度码越界）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43340 独占 0x20）。
pub const SEG_BASE: u32 = 0x43340;

const E_RANGE: u16 = 1;
const E_TIER: u16 = 2;
const E_OVER: u16 = 3;
const E_UNMEASURED: u16 = 4;
const E_DUPLICATE: u16 = 5;
const E_FULL: u16 = 6;
const E_GRADE: u16 = 7;
const E_DRIFT: u16 = 8;

/// 水印性能预算错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerfError {
    /// 作业参数越界（零尺寸 / dpi=0 / 层数 0 / 覆盖率 >100）。
    Range,
    /// dpi 不在预算表任何一档的覆盖范围内。
    Tier,
    /// 实测超过该档登记上限。
    OverBudget,
    /// 状态机顺序错误：未测量就登记。
    Unmeasured,
    /// 同 (job, grade) 已登记。
    Duplicate,
    /// 登记账容量已满。
    Full,
    /// 精度档码不在闭集 0..=2。
    Grade,
    /// 全表重算发现条目漂移（重定价把上限压到实测之下）。
    Drift,
}

impl PerfError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            PerfError::Range => E_RANGE,
            PerfError::Tier => E_TIER,
            PerfError::OverBudget => E_OVER,
            PerfError::Unmeasured => E_UNMEASURED,
            PerfError::Duplicate => E_DUPLICATE,
            PerfError::Full => E_FULL,
            PerfError::Grade => E_GRADE,
            PerfError::Drift => E_DRIFT,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            PerfError::Range => "job parameter out of range",
            PerfError::Tier => "dpi outside every budget tier",
            PerfError::OverBudget => "measured ticks exceed registered ceiling",
            PerfError::Unmeasured => "commit before sample",
            PerfError::Duplicate => "job already registered at this grade",
            PerfError::Full => "perf ledger capacity reached",
            PerfError::Grade => "grade code outside closed set",
            PerfError::Drift => "entry drifted from registered measurement",
        }
    }
}

/// 模拟器精度档。倍率写成分数而非浮点：`1/2`、`1/1`、`2/1` 在整数域精确，
/// 保证「草样 = 标准的一半」这类判据不因舍入抖动而偶发失败。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grade {
    /// 草样：字形预栅格 + 半分辨率光栅。
    Draft,
    /// 标准：全部成本项按标定系数。
    Standard,
    /// 打样：双倍采样（抗锯齿 + 边缘重绘）。
    Proof,
}

impl Grade {
    /// 档码 0..=2（落账与外部 API 的交换码）。
    pub fn code(self) -> u8 {
        match self {
            Grade::Draft => 0,
            Grade::Standard => 1,
            Grade::Proof => 2,
        }
    }

    /// 档码反解；闭集外返回 [`PerfError::Grade`]，不兜底到标准档。
    pub fn from_code(c: u8) -> Result<Grade, PerfError> {
        match c {
            0 => Ok(Grade::Draft),
            1 => Ok(Grade::Standard),
            2 => Ok(Grade::Proof),
            _ => Err(PerfError::Grade),
        }
    }

    /// 成本倍率（分子, 分母）：草样 1/2、标准 1/1、打样 2/1。
    pub fn scale(self) -> (u64, u64) {
        match self {
            Grade::Draft => (1, 2),
            Grade::Standard => (1, 1),
            Grade::Proof => (2, 1),
        }
    }

    /// 档名（日志用）。
    pub fn label(self) -> &'static str {
        match self {
            Grade::Draft => "draft",
            Grade::Standard => "standard",
            Grade::Proof => "proof",
        }
    }

    /// 全部三档（遍历用）。
    pub const ALL: [Grade; 3] = [Grade::Draft, Grade::Standard, Grade::Proof];
}

/// 光栅化成本：每 1000 像素·层 记 1 tick。模拟器档在 600 dpi / 2 层 / A4 上
/// 标定为 ~72 ms 量级，取整后即本系数（1 tick = 1 µs）。
pub const RASTER_UNITS_PER_KPX: u64 = 1;

/// 字形描边成本：每字形·层 1 tick（前提是字形已预栅格化到缓存；未命中缓存
/// 的冷路径不进本模型，由 F50054 fuzz 面单独计量）。
pub const GLYPH_UNITS: u64 = 1;

/// 半透明合成成本：每 100k 像素·层·1% 覆盖记 1 tick。
pub const BLEND_UNITS_PER_100KPCT: u64 = 1;

/// 单页预算表：`(dpi 上界, 每页 tick 上限)`。阶梯而非公式——采购合同按档签，
/// 档内插值会让「同一台机器换档」出现预算比实测还抖的情况。
/// 数值来自模拟器档标定：A4/600dpi/2 层标准档实测约 105k，取 4 倍余量。
pub const TIERS: [(u16, u64); 4] = [
    (300, 120_000),
    (600, 400_000),
    (1200, 1_500_000),
    (2400, 5_600_000),
];

/// 单页作业描述（全部整数：尺寸 mm、dpi、字形数、覆盖百分比、层数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Job {
    /// 页面宽（mm）。
    pub w_mm: u16,
    /// 页面高（mm）。
    pub h_mm: u16,
    /// 目标分辨率。
    pub dpi: u16,
    /// 本页水印字形数。
    pub glyphs: u32,
    /// 图像/半透明区域占页面积百分比（0..=100）。
    pub ink_pct: u8,
    /// 水印层数（1..=8）。
    pub layers: u8,
}

impl Job {
    /// 构造一个作业（不做校验，校验见 [`Job::valid`]）。
    pub const fn new(w_mm: u16, h_mm: u16, dpi: u16, glyphs: u32, ink_pct: u8, layers: u8) -> Job {
        Job {
            w_mm,
            h_mm,
            dpi,
            glyphs,
            ink_pct,
            layers,
        }
    }

    /// 字段闭区间校验：尺寸 10..=2000mm、dpi 72..=4800、层数 1..=8、
    /// 覆盖率 0..=100、字形数 ≤ 1e6。越界一律显式失败，不夹取——
    /// 夹取会让「参数写错」伪装成「刚好压线通过」。
    pub fn valid(&self) -> bool {
        self.w_mm >= 10
            && self.w_mm <= 2000
            && self.h_mm >= 10
            && self.h_mm <= 2000
            && self.dpi >= 72
            && self.dpi <= 4800
            && self.layers >= 1
            && self.layers <= 8
            && self.ink_pct <= 100
            && self.glyphs <= 1_000_000
    }

    /// 页像素数：`(mm × dpi / 25)²`。25 而非 25.4 是刻意的整数近似——
    /// 预算模型要可复算，不需要物理精度；偏差 <2% 已落在档位余量内。
    pub fn pixels(&self) -> Option<u64> {
        if !self.valid() {
            return None;
        }
        let w = (self.w_mm as u64) * (self.dpi as u64) / 25;
        let h = (self.h_mm as u64) * (self.dpi as u64) / 25;
        Some(w.saturating_mul(h))
    }

    /// 一行描述（落账日志用）。
    pub fn describe(&self) -> String {
        format!(
            "{}x{}mm@{}dpi g{} ink{}% L{}",
            self.w_mm, self.h_mm, self.dpi, self.glyphs, self.ink_pct, self.layers
        )
    }
}

/// 按 dpi 取该页登记上限。取**第一条** `dpi <= 上界` 的档；全部不命中即
/// [`PerfError::Tier`]——4800 dpi 的作业不该被 2400 档默默放行。
pub fn budget_for(dpi: u16) -> Result<u64, PerfError> {
    let mut i = 0usize;
    while i < TIERS.len() {
        if let Some((cap, ticks)) = TIERS.get(i) {
            if dpi <= *cap {
                return Ok(*ticks);
            }
        }
        i += 1;
    }
    Err(PerfError::Tier)
}

/// 取第 `i` 档的上限 tick（越界返回 0，判据里不会用它当真值）。
pub fn tier_ticks(i: usize) -> u64 {
    match TIERS.get(i) {
        Some((_, v)) => *v,
        None => 0,
    }
}

/// 单页合成耗时（tick）。三项成本可加且互相独立：
/// 光栅化按像素×层、字形按字形×层、半透明合成按像素×覆盖率×层。
/// 全部用 `saturating_mul`——极端参数宁可饱和到 u64 顶（必然超预算被拒），
/// 也不让 debug 构建的溢出检查把内核打挂。
pub fn measure(job: &Job, grade: Grade) -> Result<u64, PerfError> {
    let px = match job.pixels() {
        Some(v) => v,
        None => return Err(PerfError::Range),
    };
    let layers = job.layers as u64;
    let raster = px.saturating_mul(layers).saturating_mul(RASTER_UNITS_PER_KPX) / 1000;
    let text = (job.glyphs as u64)
        .saturating_mul(layers)
        .saturating_mul(GLYPH_UNITS);
    let blend = px
        .saturating_mul(job.ink_pct as u64)
        .saturating_mul(layers)
        .saturating_mul(BLEND_UNITS_PER_100KPCT)
        / 100_000;
    let base = raster.saturating_add(text).saturating_add(blend);
    let (num, den) = grade.scale();
    if den == 0 {
        return Err(PerfError::Grade);
    }
    Ok(base.saturating_mul(num) / den)
}

/// 测量状态。状态迁移是单向的（`Sealed` 之外），跳步即报错。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 空闲，未装载作业。
    Idle,
    /// 已装载作业与精度档，等待采样。
    Armed,
    /// 已采样，拿到实测值。
    Measured,
    /// 已登记上限并判定在预算内。
    Registered,
    /// 已采样但超预算，本页被拒。
    Rejected,
}

impl Stage {
    /// 是否终态（可重新装载下一张）。
    pub fn is_terminal(self) -> bool {
        matches!(self, Stage::Registered | Stage::Rejected)
    }

    /// 状态名（日志用）。
    pub fn label(self) -> &'static str {
        match self {
            Stage::Idle => "idle",
            Stage::Armed => "armed",
            Stage::Measured => "measured",
            Stage::Registered => "registered",
            Stage::Rejected => "rejected",
        }
    }
}

/// 单页测量器：`arm → sample → commit` 三步，跳步一律 [`PerfError::Unmeasured`]。
/// 为什么要状态机而不是一个 `measure()` 函数：J1 的门是「实测 ≤ 登记」，
/// 若允许「没测就登记」，账面就成了猜的；这里把顺序写成不可绕过的迁移。
#[derive(Clone, Copy, Debug)]
pub struct Meter {
    stage: Stage,
    job: Option<Job>,
    grade: Grade,
    measured: u64,
    ceiling: u64,
}

impl Meter {
    /// 新建空闲测量器。
    pub fn new() -> Meter {
        Meter {
            stage: Stage::Idle,
            job: None,
            grade: Grade::Standard,
            measured: 0,
            ceiling: 0,
        }
    }

    /// 装载作业与精度档；仅允许从空闲/终态装载（重复装载视为顺序错误）。
    pub fn arm(&mut self, job: Job, grade: Grade) -> Result<(), PerfError> {
        if !self.stage.is_terminal() && self.stage != Stage::Idle {
            return Err(PerfError::Unmeasured);
        }
        if !job.valid() {
            return Err(PerfError::Range);
        }
        self.ceiling = budget_for(job.dpi)?;
        self.job = Some(job);
        self.grade = grade;
        self.measured = 0;
        self.stage = Stage::Armed;
        Ok(())
    }

    /// 从外部档码装载（档码先过闭集校验）。
    pub fn arm_code(&mut self, job: Job, grade_code: u8) -> Result<(), PerfError> {
        let g = Grade::from_code(grade_code)?;
        self.arm(job, g)
    }

    /// 采样：必须在 `Armed` 状态，否则 [`PerfError::Unmeasured`]。
    pub fn sample(&mut self) -> Result<u64, PerfError> {
        if self.stage != Stage::Armed {
            return Err(PerfError::Unmeasured);
        }
        let job = match self.job {
            Some(j) => j,
            None => return Err(PerfError::Unmeasured),
        };
        self.measured = measure(&job, self.grade)?;
        self.stage = Stage::Measured;
        Ok(self.measured)
    }

    /// 登记：必须在 `Measured` 状态；超预算转 `Rejected` 并报
    /// [`PerfError::OverBudget`]，**不**返回一个「超了但还是登了」的成功值。
    pub fn commit(&mut self) -> Result<u64, PerfError> {
        if self.stage != Stage::Measured {
            return Err(PerfError::Unmeasured);
        }
        if self.measured > self.ceiling {
            self.stage = Stage::Rejected;
            return Err(PerfError::OverBudget);
        }
        self.stage = Stage::Registered;
        Ok(self.measured)
    }

    /// 当前状态。
    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// 本次登记的实测值。
    pub fn measured(&self) -> u64 {
        self.measured
    }

    /// 本次登记的预算上限。
    pub fn ceiling(&self) -> u64 {
        self.ceiling
    }

    /// 本次作业。
    pub fn job(&self) -> Option<Job> {
        self.job
    }
}

/// 账内一条登记：作业 + 档位 + 实测 + 上限 + 入账序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerfEntry {
    /// 作业。
    pub job: Job,
    /// 精度档。
    pub grade: Grade,
    /// 实测 tick。
    pub measured: u64,
    /// 登记上限 tick。
    pub ceiling: u64,
    /// 入账序（0 起）。
    pub seq: u16,
}

impl PerfEntry {
    /// 该条是否在预算内（J1 的逐条判据）。
    pub fn within_budget(&self) -> bool {
        self.measured <= self.ceiling
    }

    /// 余量百分比（千分比，`<=1000` 表示有富余）。
    pub fn headroom_permille(&self) -> u32 {
        if self.ceiling == 0 {
            return 0;
        }
        let h = self.ceiling.saturating_sub(self.measured);
        let p = (h as u128 * 1000) / self.ceiling as u128;
        if p > 1_000_000 {
            1_000_000
        } else {
            p as u32
        }
    }
}

/// FNV-1a 偏移基（与批内其它账本同一口径，便于横向比对）。
pub const FNV_OFFSET: u32 = 0x811c_9dc5;
/// FNV-1a 质数。
pub const FNV_PRIME: u32 = 0x0100_0193;
/// 单本性能账容量（一台设备一次标定最多 8 档 × 8 job，取 64）。
pub const MAX_ENTRIES: usize = 64;

fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ b as u32).wrapping_mul(FNV_PRIME)
}

fn fnv_u64(h: u32, v: u64) -> u32 {
    let mut h = h;
    let mut s = 56i32;
    while s >= 0 {
        h = fnv_byte(h, (v >> s) as u8);
        s -= 8;
    }
    h
}

/// 定容预算登记账。入账即比对上限，超预算的作业**不入账**（宁缺不假）。
pub struct PerfLedger {
    entries: Vec<PerfEntry>,
    capacity: usize,
    next_seq: u16,
    rejected: u32,
}

impl PerfLedger {
    /// 建空账；容量 0 按 1 处理，上限 1024 防误用。
    pub fn new(capacity: usize) -> PerfLedger {
        let cap = if capacity == 0 { 1 } else { capacity.min(1024) };
        PerfLedger {
            entries: Vec::with_capacity(cap),
            capacity: cap,
            next_seq: 0,
            rejected: 0,
        }
    }

    fn reject<T>(&mut self, e: PerfError) -> Result<T, PerfError> {
        self.rejected = self.rejected.saturating_add(1);
        Err(e)
    }

    /// 同 (job, grade) 的下标。
    pub fn find(&self, job: &Job, grade: Grade) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.job == *job && e.grade == grade {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记一条：参数校验 → 重号 → 容量 → 实测 → 比上限。返回值是实测 tick。
    pub fn register(&mut self, job: Job, grade: Grade) -> Result<u64, PerfError> {
        if !job.valid() {
            return self.reject(PerfError::Range);
        }
        let ceiling = match budget_for(job.dpi) {
            Ok(v) => v,
            Err(e) => return self.reject(e),
        };
        if self.find(&job, grade).is_some() {
            return self.reject(PerfError::Duplicate);
        }
        if self.entries.len() >= self.capacity {
            return self.reject(PerfError::Full);
        }
        let measured = match measure(&job, grade) {
            Ok(v) => v,
            Err(e) => return self.reject(e),
        };
        if measured > ceiling {
            return self.reject(PerfError::OverBudget);
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.entries.push(PerfEntry {
            job,
            grade,
            measured,
            ceiling,
            seq,
        });
        Ok(measured)
    }

    /// 重定价：预算表修订后把某条上限改成新值。改**小**到实测之下会让账本
    /// 自相矛盾，随后 [`PerfLedger::verify`] 会报 `Drift`——这条路径是
    /// 「预算表降档后必须重跑实测」的强制留痕，不是测试后门。
    pub fn rebill(&mut self, idx: usize, new_ceiling: u64) -> Result<(), PerfError> {
        match self.entries.get_mut(idx) {
            Some(e) => {
                e.ceiling = new_ceiling;
                Ok(())
            }
            None => Err(PerfError::Range),
        }
    }

    /// 全表重算：逐条重测 + 复核上限 + 复核入账序连续。任一不符即 `Drift`。
    pub fn verify(&self) -> Result<usize, PerfError> {
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = match self.entries.get(i) {
                Some(v) => v,
                None => return Err(PerfError::Drift),
            };
            let again = measure(&e.job, e.grade).map_err(|_| PerfError::Drift)?;
            if again != e.measured || !e.within_budget() {
                return Err(PerfError::Drift);
            }
            if e.seq as usize != i {
                return Err(PerfError::Drift);
            }
            i += 1;
        }
        Ok(self.entries.len())
    }

    /// 账面指纹：入账序 → 字段序（作业 / 档 / 实测 / 上限）。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = fnv_byte(h, e.job.w_mm as u8);
                h = fnv_byte(h, (e.job.w_mm >> 8) as u8);
                h = fnv_byte(h, e.job.h_mm as u8);
                h = fnv_byte(h, (e.job.h_mm >> 8) as u8);
                h = fnv_byte(h, e.job.dpi as u8);
                h = fnv_byte(h, (e.job.dpi >> 8) as u8);
                h = fnv_byte(h, e.job.layers);
                h = fnv_byte(h, e.grade.code());
                h = fnv_u64(h, e.measured);
                h = fnv_u64(h, e.ceiling);
            }
            i += 1;
        }
        h
    }

    /// 账面条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 累计被拒次数（显性记账，不静默吞）。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 取第 `i` 条（越界 `None`）。
    pub fn entry(&self, i: usize) -> Option<&PerfEntry> {
        self.entries.get(i)
    }

    /// 实测总和。
    pub fn total_measured(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                sum = sum.saturating_add(e.measured);
            }
            i += 1;
        }
        sum
    }

    /// 上限总和。
    pub fn total_ceiling(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                sum = sum.saturating_add(e.ceiling);
            }
            i += 1;
        }
        sum
    }

    /// 是否每条都在预算内（J1 的整账判据）。
    pub fn all_within_budget(&self) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            match self.entries.get(i) {
                Some(e) if e.within_budget() => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

/// 标准作业集：A4/A5 两条轻档（300/600 dpi）+ A3 两条重档（1200 dpi）。
/// 轻档用来证明「实测 ≤ 登记」，重档里 4 层厚涂那条用来证明「越预算必拒」。
pub fn standard_jobs() -> Vec<Job> {
    let mut v = Vec::with_capacity(4);
    v.push(Job::new(210, 297, 300, 4000, 35, 2));
    v.push(Job::new(148, 210, 600, 2000, 30, 1));
    v.push(Job::new(297, 420, 1200, 4000, 35, 2));
    v.push(Job::new(297, 420, 1200, 4000, 60, 4));
    v
}

/// 注入核：7 类坏样本，逐类断言被显式拒绝。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 7usize;

    // 1) 零宽度：不是「很小」而是非法，必须 Range。
    let zero = Job::new(0, 297, 600, 100, 10, 1);
    if measure(&zero, Grade::Standard).err() == Some(PerfError::Range) {
        caught += 1;
    }

    // 2) dpi=0 同理非法（pixels() 里会退化成 0，若放行则实测恒 0「永远达标」）。
    let dpi0 = Job::new(210, 297, 0, 100, 10, 1);
    if measure(&dpi0, Grade::Standard).err() == Some(PerfError::Range) {
        caught += 1;
    }

    // 3) 未知档位：4800 dpi 超过最高档 2400，必须 Tier 而不是按 2400 档放行。
    if budget_for(4800).err() == Some(PerfError::Tier) {
        caught += 1;
    }

    // 4) 越预算：1200 dpi × 4 层 × 60% 覆盖的超预算作业必须被拒，且账长不变。
    let mut ledger = PerfLedger::new(8);
    let heavy = Job::new(297, 420, 1200, 4000, 60, 4);
    let registered = ledger.register(Job::new(210, 297, 300, 4000, 35, 2), Grade::Standard).is_ok();
    if registered
        && ledger.register(heavy, Grade::Standard).err() == Some(PerfError::OverBudget)
        && ledger.len() == 1
        && ledger.rejected() == 1
    {
        caught += 1;
    }

    // 5) 未测量就登记：状态机跳步必须 Unmeasured。
    let mut m = Meter::new();
    if m.commit().err() == Some(PerfError::Unmeasured) {
        caught += 1;
    }

    // 6) 重号：同一 job 同一档登记两次必须 Duplicate。
    let job = Job::new(210, 297, 600, 4000, 35, 2);
    let mut dup = PerfLedger::new(8);
    let first = dup.register(job, Grade::Standard).is_ok();
    if first && dup.register(job, Grade::Standard).err() == Some(PerfError::Duplicate) {
        caught += 1;
    }

    // 7) 档码越界：闭集外的 7 必须 Grade 错误（外部 API 直传档码的防线）。
    let mut m2 = Meter::new();
    if m2.arm_code(job, 7).err() == Some(PerfError::Grade) {
        caught += 1;
    }

    (caught, total)
}

/// UNX-F50057 自检：14 条判据。
pub fn run_unx50057_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50057");
    let jobs = standard_jobs();

    // J1-1 主判据「实测 ≤ 预算登记」：轻档两条逐条登记且实测不超上限。
    let mut book = PerfLedger::new(MAX_ENTRIES);
    let mut registered = 0usize;
    let mut i = 0usize;
    while i < 2 {
        if let Some(j) = jobs.get(i) {
            if book.register(*j, Grade::Standard).is_ok() {
                registered += 1;
            }
        }
        i += 1;
    }
    let ok = jobs.len() == 4
        && registered == 2
        && book.all_within_budget()
        && book.total_measured() <= book.total_ceiling();
    let det = format!(
        "n={} m={} c={}",
        book.len(),
        book.total_measured(),
        book.total_ceiling()
    );
    s.add_dyn("50057-J1-实测不超登记", ok, &det);

    // J1-2 面积单调：宽度递增 → 实测不减（成本项对像素线性、像素对宽度线性）。
    let base = Job::new(210, 297, 600, 0, 40, 1);
    let wide = Job::new(420, 297, 600, 0, 40, 1);
    let ok = match (measure(&base, Grade::Standard), measure(&wide, Grade::Standard)) {
        (Ok(a), Ok(b)) => b > a,
        _ => false,
    };
    let det = match (measure(&base, Grade::Standard), measure(&wide, Grade::Standard)) {
        (Ok(a), Ok(b)) => format!("a={} b={}", a, b),
        _ => "measure failed".to_string(),
    };
    s.add_dyn("50057-J1-面积单调", ok, &det);

    // J1-3 层数单调：多加一层只会更贵（三项成本都乘层数）。
    let l1 = Job::new(210, 297, 600, 4000, 35, 1);
    let l4 = Job::new(210, 297, 600, 4000, 35, 4);
    let ok = match (measure(&l1, Grade::Standard), measure(&l4, Grade::Standard)) {
        (Ok(a), Ok(b)) => b > a,
        _ => false,
    };
    s.add("50057-J1-层数单调", ok, if ok { "" } else { "层数增加成本未增" });

    // J1-4 档位阶梯：draft ≤ standard ≤ proof，且 draft 恰为标准的一半（整数）。
    let mut ladder = true;
    let mut k = 0usize;
    while k < jobs.len() {
        if let Some(j) = jobs.get(k) {
            let d = measure(j, Grade::Draft);
            let st = measure(j, Grade::Standard);
            let pr = measure(j, Grade::Proof);
            match (d, st, pr) {
                (Ok(d), Ok(st), Ok(pr)) => {
                    if !(d <= st && st <= pr && d * 2 <= st + 1 && st * 2 == pr) {
                        ladder = false;
                    }
                }
                _ => ladder = false,
            }
        }
        k += 1;
    }
    s.add("50057-J1-档位阶梯", ladder, if ladder { "" } else { "档位倍率不成立" });

    // J1-5 确定性：同 job 同档两次测量逐位一致。
    let mut det_ok = true;
    k = 0;
    while k < jobs.len() {
        if let Some(j) = jobs.get(k) {
            if measure(j, Grade::Proof) != measure(j, Grade::Proof) {
                det_ok = false;
            }
        }
        k += 1;
    }
    s.add("50057-J1-测量确定", det_ok, if det_ok { "" } else { "两次测量不等" });

    // J1-6 越预算必拒，且不入账（账长与 rejected 都要对得上）。
    let heavy = match jobs.get(3) {
        Some(j) => *j,
        None => Job::new(297, 420, 1200, 4000, 60, 4),
    };
    let before = book.len();
    let rej = book.register(heavy, Grade::Standard);
    let ok = rej.err() == Some(PerfError::OverBudget) && book.len() == before && book.rejected() == 1;
    let m_heavy = match measure(&heavy, Grade::Standard) {
        Ok(v) => v,
        Err(_) => 0,
    };
    let c_heavy = match budget_for(heavy.dpi) {
        Ok(v) => v,
        Err(_) => 0,
    };
    let det = format!("m={} cap={}", m_heavy, c_heavy);
    s.add_dyn("50057-J1-越预算必拒", ok, &det);

    // J1-7 未测量不得登记：空测量器直接 commit 必须 Unmeasured。
    let mut m = Meter::new();
    let ok = m.commit().err() == Some(PerfError::Unmeasured) && m.stage() == Stage::Idle;
    s.add("50057-J1-未测不登记", ok, if ok { "" } else { "未测也登记成功" });

    // J1-8 状态机正路：arm → sample → commit 得 Registered，且值与模型一致。
    let light = Job::new(210, 297, 300, 4000, 35, 2);
    let mut m2 = Meter::new();
    let armed = m2.arm(light, Grade::Standard).is_ok();
    let sampled = m2.sample();
    let committed = m2.commit();
    let ok = armed
        && sampled == measure(&light, Grade::Standard)
        && committed == sampled
        && m2.stage() == Stage::Registered
        && m2.measured() <= m2.ceiling();
    let det = format!("stage={} v={}", m2.stage().label(), m2.measured());
    s.add_dyn("50057-J1-状态机正路", ok, &det);

    // J1-9 重复装载/重号：同条二次登记必 Duplicate，指纹不变。
    let dg_before = book.digest();
    let again = book.register(light, Grade::Standard);
    let ok = again.err() == Some(PerfError::Duplicate) && book.digest() == dg_before;
    s.add("50057-J1-重号必拒", ok, if ok { "" } else { "重复登记改了账" });

    // J1-10 定容不破：容量 2 的账收满后第三条 Full。
    let mut small = PerfLedger::new(2);
    let a = Job::new(100, 150, 300, 10, 10, 1);
    let b = Job::new(110, 150, 300, 10, 10, 1);
    let c = Job::new(120, 150, 300, 10, 10, 1);
    let ok = small.register(a, Grade::Draft).is_ok()
        && small.register(b, Grade::Draft).is_ok()
        && small.register(c, Grade::Draft).err() == Some(PerfError::Full)
        && small.len() == small.capacity();
    let det = format!("n={} cap={}", small.len(), small.capacity());
    s.add_dyn("50057-J1-定容不破", ok, &det);

    // J1-11 参数越界四连：零宽 / dpi=0 / 层数 0 / 覆盖率 101。
    let bad = [
        Job::new(0, 297, 600, 100, 10, 1),
        Job::new(210, 297, 0, 100, 10, 1),
        Job::new(210, 297, 600, 100, 10, 0),
        Job::new(210, 297, 600, 100, 101, 1),
    ];
    let mut all_bad = true;
    let mut j = 0usize;
    while j < bad.len() {
        if let Some(bj) = bad.get(j) {
            if bj.valid() || measure(bj, Grade::Standard).err() != Some(PerfError::Range) {
                all_bad = false;
            }
        }
        j += 1;
    }
    s.add("50057-J1-越界必拒", all_bad, if all_bad { "" } else { "越界参数被接受" });

    // J1-12 档位边界：300/600/1200/2400 各落各自档，2401 落 2400 档之外必须 Tier。
    let tier_ok = budget_for(300) == Ok(tier_ticks(0))
        && budget_for(301) == Ok(tier_ticks(1))
        && budget_for(1200) == Ok(tier_ticks(2))
        && budget_for(2400) == Ok(tier_ticks(3))
        && budget_for(2401).err() == Some(PerfError::Tier);
    s.add("50057-J1-档位边界", tier_ok, if tier_ok { "" } else { "档位归属错" });

    // J1-13 账守恒 + 重定价留痕：Σ实测 ≤ Σ上限、全表重算通过；
    // 把某条上限压到实测之下后 verify 必报 Drift（不是静默通过）。
    let mut audit = PerfLedger::new(MAX_ENTRIES);
    let _ = audit.register(Job::new(210, 297, 300, 4000, 35, 2), Grade::Standard);
    let _ = audit.register(Job::new(148, 210, 600, 2000, 30, 1), Grade::Proof);
    let conserved = audit.len() == 2
        && audit.total_measured() <= audit.total_ceiling()
        && audit.verify() == Ok(2)
        && audit.digest() != FNV_OFFSET;
    let rebilling = audit.rebill(1, 1);
    let drifted = audit.verify().err() == Some(PerfError::Drift);
    let ok = conserved && rebilling.is_ok() && drifted;
    let det = format!(
        "m={} c={} drift={}",
        audit.total_measured(),
        audit.total_ceiling(),
        drifted
    );
    s.add_dyn("50057-J1-账守恒留痕", ok, &det);

    // J1-14 七类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50057-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50057_checks_green() {
        let set = run_unx50057_checks();
        assert!(set.all_passed(), "UNX-F50057 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50057_budget_ladder_is_integer() {
        let j = Job::new(210, 297, 600, 4000, 35, 2);
        let d = measure(&j, Grade::Draft);
        let st = measure(&j, Grade::Standard);
        let pr = measure(&j, Grade::Proof);
        assert!(d.is_ok() && st.is_ok() && pr.is_ok());
        if let (Ok(d), Ok(st), Ok(pr)) = (d, st, pr) {
            // 草样 = base/2（整除舍尾），打样 = base*2。
            assert!(d * 2 + st % 2 == st);
            assert_eq!(pr, st * 2);
            assert_eq!(d, st / 2);
        }
    }

    #[test]
    fn unx50057_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 7);
    }
}
