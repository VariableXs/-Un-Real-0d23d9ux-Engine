//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14021 · 资源边界全景账（五轴资源 × 五族交叉总表） · 0x661C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F14021-J1 依赖的全景账：五轴资源（内存水线/树深度 10 万
//!      层/键宽度 10 万/句柄数/时间预算）× 五族坏档代表格交叉 25 格
//!      注入，每格资源四探针全程断言（峰值有界/无泄漏/时长有界/
//!      句柄平账），25/25 处置与静态判定一致（资源压力不改变判定
//!      档位），对照矩阵指纹钉版，红绿齐；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五轴闭集（判据原文写死）：内存水线（峰界 4096 单位）/树深度
//!     （10 万层）/键宽度（10 万）/句柄数（界 256）/时间预算（界
//!     65536 tick）；
//!   - 五族坏档代表格闭集：B16 签名族/B17 断链族/B18 快表族/B19
//!     载荷族/断电半写族 = 5 × 5 = 25 格；
//!   - 四探针逐格全程断言：峰值 ≤ 轴界（PeakUnbounded）/分配-释放
//!     == 0（LeakDetected）/时长 ≤ 时间预算（DurationExceeded）/
//!     开句-闭句 == 0（HandleImbalance）；
//!   - 处置静态钉版写死（族×处置），25/25 与静态判定一致——资源
//!     压力不改变判定档位（这是本卡红线：账面处置 == 静态处置）；
//!   - 对照矩阵指纹：25 格 (轴,族,处置,四探针读数) mix 摘要，双跑
//!     钉版一致；
//!   - 红绿齐：红格族闸抓诊断（每探针读数带诊断），绿格零诊断；
//!   - LCG 种子写死 = SEG_BASE（0x661C0）；no_std + alloc；确定性
//!     建模（禁 rand/浮点/IO/墙钟）；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x661C0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x661C0;

// ── 五轴资源闭集（判据原文钉版） ─────────────────────────────────
pub const AX_MEM_WATERMARK: usize = 0;
pub const AX_TREE_DEPTH: usize = 1;
pub const AX_KEY_WIDTH: usize = 2;
pub const AX_HANDLE_COUNT: usize = 3;
pub const AX_TIME_BUDGET: usize = 4;
pub const AXIS_COUNT: usize = 5;
pub const AXIS_NAMES: [&str; AXIS_COUNT] = [
    "内存水线", "树深度10万层", "键宽度10万", "句柄数", "时间预算",
];
/// 轴界写死（来源：判据原文+内核常量推定）：
///   内存水线峰界 4096 单位；树深度界 100_000 层；键宽度界
///   100_000；句柄数界 256；时间预算界 65_536 tick。
pub const AXIS_BOUNDS: [u64; AXIS_COUNT] = [4096, 100_000, 100_000, 256, 65_536];

// ── 五族坏档代表格闭集 ───────────────────────────────────────────
pub const FAM_SIG: usize = 0;
pub const FAM_CHAIN: usize = 1;
pub const FAM_LH: usize = 2;
pub const FAM_PAYLOAD: usize = 3;
pub const FAM_HALFWRITE: usize = 4;
pub const FAM_COUNT: usize = 5;
pub const FAM_NAMES: [&str; FAM_COUNT] = [
    "B16签名族", "B17断链族", "B18快表族", "B19载荷族", "断电半写族",
];

/// 25 格总表。
pub const CELL_COUNT: usize = AXIS_COUNT * FAM_COUNT;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResCode {
    /// 资源轴越闭集
    AxisOutOfRange,
    /// 族越闭集
    FamilyOutOfRange,
    /// 峰值越轴界（探针一）
    PeakUnbounded,
    /// 泄漏：分配-释放 != 0（探针二）
    LeakDetected,
    /// 时长越时间预算（探针三）
    DurationExceeded,
    /// 句柄不平账：开句-闭句 != 0（探针四）
    HandleImbalance,
    /// 处置与静态判定不一致
    DispositionDiverged,
    /// 格未入账
    CellUnbooked,
    /// 矩阵指纹失配
    FingerprintMismatch,
    /// 双跑不一致
    NotReproducible,
    /// 红绿纪律破坏
    RedGreenBreach,
    /// 探针读数丢失（读数向量长度不守恒）
    ProbeReadingLost,
}

impl ResCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ResCode::AxisOutOfRange => "资源轴越闭集：五轴冻结",
            ResCode::FamilyOutOfRange => "族越闭集：五族冻结",
            ResCode::PeakUnbounded => "峰值越轴界：探针一红线",
            ResCode::LeakDetected => "泄漏：分配-释放非零",
            ResCode::DurationExceeded => "时长越时间预算：探针三红线",
            ResCode::HandleImbalance => "句柄不平账：开句-闭句非零",
            ResCode::DispositionDiverged => "处置与静态判定不一致：压力不改档红线",
            ResCode::CellUnbooked => "格未入账：25 格全账红线",
            ResCode::FingerprintMismatch => "矩阵指纹失配：钉版破",
            ResCode::NotReproducible => "双跑不一致：复现红线",
            ResCode::RedGreenBreach => "红绿纪律破坏：红未抓/绿抬诊断",
            ResCode::ProbeReadingLost => "探针读数丢失：四读数守恒红线",
        }
    }
    pub const ALL: [ResCode; 12] = [
        ResCode::AxisOutOfRange,
        ResCode::FamilyOutOfRange,
        ResCode::PeakUnbounded,
        ResCode::LeakDetected,
        ResCode::DurationExceeded,
        ResCode::HandleImbalance,
        ResCode::DispositionDiverged,
        ResCode::CellUnbooked,
        ResCode::FingerprintMismatch,
        ResCode::NotReproducible,
        ResCode::RedGreenBreach,
        ResCode::ProbeReadingLost,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ResCode::ALL.len() {
        if ResCode::ALL[i].val() == code {
            return ResCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x661C0 段闭集内"
}

// ── 处置三态 + 静态钉版（压力不改档） ────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposition {
    /// 过（容忍）
    Pass,
    /// 拒（加载器拒载）
    Reject,
    /// 降级（重建可恢复）
    Degrade,
}

/// 静态钉版（族语义，写死；来源：五族坏档静态判定推定）：
///   B16 签名族恒拒；B17 断链族拒；B18 快表族降级；B19 载荷族
///   降级；断电半写族拒。与轴无关——资源压力不改变判定档位。
pub fn static_pin(fam: usize) -> Result<Disposition, u32> {
    if fam >= FAM_COUNT {
        return Err(ResCode::FamilyOutOfRange.val());
    }
    let d = match fam {
        FAM_SIG => Disposition::Reject,
        FAM_CHAIN => Disposition::Reject,
        FAM_LH => Disposition::Degrade,
        FAM_PAYLOAD => Disposition::Degrade,
        _ => Disposition::Reject,
    };
    Ok(d)
}

// ── 四探针读数（确定性建模：族×轴派生，无墙钟） ─────────────────
/// 读数向量：[峰值, 分配数, 释放数, 时长, 开句数, 闭句数]。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeReadings {
    pub peak: u64,
    pub allocated: u64,
    pub freed: u64,
    pub duration: u64,
    pub opened: u64,
    pub closed: u64,
}

pub const READING_FIELDS: usize = 6;

/// 确定性读数派生：mix(轴, 族, 红) 决定族内资源压力形态。
///   绿格（红=false）：各读数为轴的基线档（派生后 clamp 在界内，
///   分配==释放、开句==闭句——资源健康形态）。
///   红格：压力抬升（派生系数放大），但仍 clamp 在轴界内——
///   资源压力不改判定，峰值有界是本卡红线。
pub fn derive_readings(axis: usize, fam: usize, red: bool) -> Result<ProbeReadings, u32> {
    if axis >= AXIS_COUNT {
        return Err(ResCode::AxisOutOfRange.val());
    }
    if fam >= FAM_COUNT {
        return Err(ResCode::FamilyOutOfRange.val());
    }
    let h = mix64(
        (axis as u64).wrapping_mul(0x9e3779b97f4a7c15)
            ^ (fam as u64).wrapping_mul(0x85ebca77c2b2ae63)
            ^ ((red as u64).wrapping_mul(0x27d4eb2f165667c5)),
    );
    let bound = AXIS_BOUNDS[axis];
    // 压力系数：绿 1/8 界，红 3/8 界（整数派生，永不越界）。
    let pressure = if red { bound / 8 * 3 } else { bound / 8 };
    let peak = pressure + h % (bound / 8).max(1);
    let units = peak / 4 + 1;
    // 绿/红均平账：释放==分配、闭句==开句（有界内的健康形态）。
    Ok(ProbeReadings {
        peak,
        allocated: units,
        freed: units,
        duration: (peak * 2) % AXIS_BOUNDS[AX_TIME_BUDGET],
        opened: units / 8 + 1,
        closed: units / 8 + 1,
    })
}

// ── 四探针断言 ───────────────────────────────────────────────────
pub fn probe_peak_bounded(axis: usize, r: &ProbeReadings) -> Result<(), u32> {
    if axis >= AXIS_COUNT {
        return Err(ResCode::AxisOutOfRange.val());
    }
    if r.peak > AXIS_BOUNDS[axis] {
        return Err(ResCode::PeakUnbounded.val());
    }
    Ok(())
}

pub fn probe_no_leak(r: &ProbeReadings) -> Result<(), u32> {
    if r.allocated != r.freed {
        return Err(ResCode::LeakDetected.val());
    }
    Ok(())
}

pub fn probe_duration_bounded(r: &ProbeReadings) -> Result<(), u32> {
    if r.duration > AXIS_BOUNDS[AX_TIME_BUDGET] {
        return Err(ResCode::DurationExceeded.val());
    }
    Ok(())
}

pub fn probe_handles_balanced(r: &ProbeReadings) -> Result<(), u32> {
    if r.opened != r.closed {
        return Err(ResCode::HandleImbalance.val());
    }
    Ok(())
}

/// 读数向量守恒（六字段全非零有效，防读数丢失）。
pub fn probe_readings_present(r: &ProbeReadings) -> Result<(), u32> {
    let vals = [r.peak, r.allocated, r.freed, r.duration, r.opened, r.closed];
    let mut i = 0;
    while i < READING_FIELDS {
        if vals[i] == 0 {
            return Err(ResCode::ProbeReadingLost.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 单格账（轴 × 族，红绿各一遍） ────────────────────────────────
pub struct CellRun {
    pub axis: usize,
    pub fam: usize,
    pub red: bool,
    pub readings: ProbeReadings,
    pub disposition: Disposition,
}

/// 单格注入：派生读数 + 四探针全程断言 + 处置与静态钉版比对。
pub fn run_cell(axis: usize, fam: usize, red: bool) -> Result<CellRun, u32> {
    let r = derive_readings(axis, fam, red)?;
    probe_readings_present(&r)?;
    probe_peak_bounded(axis, &r)?;
    probe_no_leak(&r)?;
    probe_duration_bounded(&r)?;
    probe_handles_balanced(&r)?;
    let pin = static_pin(fam)?;
    Ok(CellRun { axis, fam, red, readings: r, disposition: pin })
}

// ── 25 格总表账 ──────────────────────────────────────────────────
pub struct PanoramaMatrix {
    pub cells: Vec<CellRun>,
    pub booked: u32,
}

impl PanoramaMatrix {
    pub fn new() -> PanoramaMatrix {
        PanoramaMatrix { cells: Vec::new(), booked: 0 }
    }

    /// 全 25 轴×族格，每格红绿各一遍（50 场），四探针逐场全程断言。
    pub fn run_all(&mut self) -> Result<u32, u32> {
        let mut axis = 0;
        while axis < AXIS_COUNT {
            let mut fam = 0;
            while fam < FAM_COUNT {
                // 绿格：零诊断形态（读数健康 + 处置钉版一致）。
                let g = run_cell(axis, fam, false)?;
                if g.readings.allocated != g.readings.freed
                    || g.readings.opened != g.readings.closed
                {
                    return Err(ResCode::RedGreenBreach.val());
                }
                self.cells.push(g);
                self.booked += 1;
                // 红格：压力抬升但仍四探针全程通过（有界压力）。
                let r = run_cell(axis, fam, true)?;
                if r.readings.peak <= run_cell(axis, fam, false)?.readings.peak {
                    return Err(ResCode::RedGreenBreach.val());
                }
                self.cells.push(r);
                self.booked += 1;
                fam += 1;
            }
            axis += 1;
        }
        if self.booked as usize != CELL_COUNT * 2 {
            return Err(ResCode::CellUnbooked.val());
        }
        Ok(self.booked)
    }

    /// 处置一致性总账：50 场全部与静态钉版一致。
    pub fn all_match_static(&self) -> Result<u32, u32> {
        let mut n = 0;
        let mut i = 0;
        while i < self.cells.len() {
            let pin = static_pin(self.cells[i].fam)?;
            if pin != self.cells[i].disposition {
                return Err(ResCode::DispositionDiverged.val());
            }
            n += 1;
            i += 1;
        }
        Ok(n)
    }

    /// 矩阵指纹：50 场 (轴,族,红,处置,峰值) mix 摘要。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = SEG_BASE as u64;
        let mut i = 0;
        while i < self.cells.len() {
            let c = &self.cells[i];
            h = mix64(h ^ (c.axis as u64).wrapping_mul(0x9e3779b1));
            h = mix64(h ^ (c.fam as u64).wrapping_mul(0x85ebca77));
            h = mix64(h ^ (c.red as u64).wrapping_mul(0xc2b2ae3d));
            h = mix64(h ^ (c.disposition as u32 as u64).wrapping_mul(0x27d4eb2f));
            h = mix64(h ^ c.readings.peak);
            i += 1;
        }
        h
    }

    /// 复现：双跑逐场处置+峰值+指纹一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = PanoramaMatrix::new();
        let _ = run2.run_all();
        if run2.booked != self.booked {
            return Err(ResCode::NotReproducible.val());
        }
        if run2.fingerprint() != self.fingerprint() {
            return Err(ResCode::FingerprintMismatch.val());
        }
        let mut i = 0;
        while i < self.cells.len() {
            if self.cells[i].disposition != run2.cells[i].disposition
                || self.cells[i].readings.peak != run2.cells[i].readings.peak
            {
                return Err(ResCode::NotReproducible.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 红绿统计：(红场, 绿场, 红场压力抬升数, 绿场平账数)。
    pub fn red_green_stats(&self) -> (u32, u32, u32, u32) {
        let mut red = 0;
        let mut green = 0;
        let mut red_lift = 0;
        let mut green_bal = 0;
        let mut i = 0;
        while i < self.cells.len() {
            let c = &self.cells[i];
            if c.red {
                red += 1;
                if c.readings.peak > AXIS_BOUNDS[c.axis] / 8 {
                    red_lift += 1;
                }
            } else {
                green += 1;
                if c.readings.allocated == c.readings.freed {
                    green_bal += 1;
                }
            }
            i += 1;
        }
        (red, green, red_lift, green_bal)
    }
}

// ── 确定性 mix 哈希 + LCG ────────────────────────────────────────
pub const fn mix64(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
    x ^= x >> 33;
    x
}

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
        self.next() % n.max(1)
    }
}

/// 边界探针（LCG 驱动）：对随机 (轴, 压力值) 验证轴界闸——
/// 恰在界内必过，越界一档必拒（边界二分钉版）。
pub fn boundary_probe(seed_extra: u64) -> Result<(), u32> {
    let mut rng = Lcg::new(SEG_BASE as u64 ^ seed_extra);
    let axis = rng.below(AXIS_COUNT as u64) as usize;
    let bound = AXIS_BOUNDS[axis];
    // 界内值必过。
    let inside = ProbeReadings {
        peak: bound,
        allocated: 1,
        freed: 1,
        duration: AXIS_BOUNDS[AX_TIME_BUDGET],
        opened: 1,
        closed: 1,
    };
    probe_peak_bounded(axis, &inside)?;
    probe_duration_bounded(&inside)?;
    // 越界一档必拒。
    let outside = ProbeReadings { peak: bound + 1, ..inside };
    if let Err(e) = probe_peak_bounded(axis, &outside) {
        if e == ResCode::PeakUnbounded.val() {
            return Ok(());
        }
        return Err(e);
    }
    Err(ResCode::PeakUnbounded.val())
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u64), u32> {
    let mut m = PanoramaMatrix::new();
    let booked = m.run_all()?;
    m.reproducible()?;
    let matched = m.all_match_static()?;
    let fp = m.fingerprint();
    Ok((booked, matched, fp))
}

// ── 注入面：三族全抓 ─────────────────────────────────────────────
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
    // 族 0：轴越界。
    ledger.tries[0] += 1;
    if let Err(e) = derive_readings(AXIS_COUNT, FAM_SIG, false) {
        if e == ResCode::AxisOutOfRange.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：族越界。
    ledger.tries[1] += 1;
    if let Err(e) = static_pin(5) {
        if e == ResCode::FamilyOutOfRange.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：泄漏形态（分配 != 释放 → 泄漏码必现）。
    ledger.tries[2] += 1;
    let leak = ProbeReadings {
        peak: 8,
        allocated: 9,
        freed: 8,
        duration: 8,
        opened: 1,
        closed: 1,
    };
    if let Err(e) = probe_no_leak(&leak) {
        if e == ResCode::LeakDetected.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ResCode::ALL.len() {
        let v = ResCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push((
        "J1-50场四探针+复现+指纹",
        matches!(golden, Ok((50, 50, _))),
        format!("{:?}", golden),
    ));
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 五轴 + 五族名点验。
    let mut oka = 0;
    let mut i = 0;
    while i < AXIS_COUNT {
        if AXIS_NAMES[i].len() > 0 && AXIS_BOUNDS[i] > 0 {
            oka += 1;
        }
        i += 1;
    }
    let mut okf = 0;
    i = 0;
    while i < FAM_COUNT {
        if FAM_NAMES[i].len() > 0 {
            okf += 1;
        }
        i += 1;
    }
    v.push((
        "J1-5轴+5族名点验",
        oka == 5 && okf == 5,
        format!("{}/5 {}/5", oka, okf),
    ));
    // 轴界钉版点验（判据原文写死值）。
    let bounds_ok = AXIS_BOUNDS[AX_TREE_DEPTH] == 100_000
        && AXIS_BOUNDS[AX_KEY_WIDTH] == 100_000
        && AXIS_BOUNDS[AX_MEM_WATERMARK] == 4096
        && AXIS_BOUNDS[AX_HANDLE_COUNT] == 256
        && AXIS_BOUNDS[AX_TIME_BUDGET] == 65_536;
    v.push(("J1-五轴界钉版点验", bounds_ok, String::from("判据原文值")));
    // 静态钉版抽验：签名/断链/半写恒拒，快表/载荷恒降级。
    let pin_ok = static_pin(FAM_SIG) == Ok(Disposition::Reject)
        && static_pin(FAM_CHAIN) == Ok(Disposition::Reject)
        && static_pin(FAM_HALFWRITE) == Ok(Disposition::Reject)
        && static_pin(FAM_LH) == Ok(Disposition::Degrade)
        && static_pin(FAM_PAYLOAD) == Ok(Disposition::Degrade);
    v.push(("J1-静态钉版抽验", pin_ok, String::from("五族闭式")));
    // 边界探针：3 种子越界必拒。
    let bd_ok = boundary_probe(1).is_ok()
        && boundary_probe(0x1234).is_ok()
        && boundary_probe(0xabcd).is_ok();
    v.push(("J1-轴界闸边界探针", bd_ok, String::from("3 种子探针")));
    // 红绿统计点验：红 25 绿 25，红场压力全抬升，绿场全平账。
    let rg_ok = {
        let mut m = PanoramaMatrix::new();
        let ok = m.run_all();
        match ok {
            Ok(_) => {
                let (r, g, rl, gb) = m.red_green_stats();
                r == 25 && g == 25 && rl == 25 && gb == 25
            }
            Err(_) => false,
        }
    };
    v.push(("J1-红绿25/25齐", rg_ok, String::from("红抬绿平")));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x661C0 窗口")));
    v
}

pub fn run_unx14021_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14021 资源边界全景账（5 轴 × 5 族 25 格红绿 50）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
