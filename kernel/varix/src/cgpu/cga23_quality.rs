//! CGPU-F0023 · CPU 栅格化质量档与对拍（cga23_quality）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0023`
//! **判据原文摘录**：对拍差异入册且 ≤阈值、质量档同语义、降级联动、全混合模式
//! 覆盖、回归门集成。
//!
//! 回退不等于降质偷懒。CPU 路径必须能**证明**自己和 GPU 路径同水准，而不是
//! 靠「代码看起来一样」。本模块就是那条证明链，四段可判：
//!
//! 1. **质量档**：`Profile::for_tier` 是唯一构造入口，档位只决定参数组合，
//!    参数语义与 GPU 侧同名枚举一一对应（AA 采样数 / 过滤抽头 / 精度位宽 /
//!    渲染尺度）。容差**由参数推导**而不是手写常量表——手写表最容易出现的
//!    事故是「改了 L4 的尺度忘了改容差」，那种漂移没有任何东西会报警。
//! 2. **对拍**：逐像素逐通道比 CPU 与 GPU 的产物，报 `over`（超阈像素数）、
//!    `max_diff`、`mean_milli`、`first_over`（首个超阈像素序号）。判据用
//!    **最大值**而非均值：±2 交替噪声的均值是 0，但它是实打实的差异。
//! 3. **入册**：`DiffLedger` 滚动记录（档位 × 混合模式 × 报告），红项按
//!    「(档位, 模式)」列出——不是「N/M 绿」这种没法定位的汇总。
//! 4. **回归门**：全部档位无红 **且** 24 式混合模式全覆盖才放行。没比过的
//!    模式等于没验过，未覆盖面必须挡住门，否则「覆盖」只是个数字。
//!
//! 错误路径（0xD0 段）：`E_Q_TIER` 档位越界 / `E_Q_DIM` 尺寸非法或两图不等 /
//! `E_Q_SHORT` 缓冲不足 / `E_Q_MODE` 混合模式越界 / `E_Q_GATE` 门禁拒绝。

use alloc::vec;
use super::cga16_simdblend::{BlendMode, MODE_COUNT};
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xD000;
/// 档位枚举越界。
pub const E_Q_TIER: u16 = SEG_BASE + 1;
/// 尺寸非法或两图尺寸不等。
pub const E_Q_DIM: u16 = SEG_BASE + 2;
/// 像素缓冲不足。
pub const E_Q_SHORT: u16 = SEG_BASE + 3;
/// 混合模式越界。
pub const E_Q_MODE: u16 = SEG_BASE + 4;
/// 回归门拒绝（红项名单随 `GateResult` 返回）。
pub const E_Q_GATE: u16 = SEG_BASE + 5;

/// 无超阈像素时的哨兵序号。
pub const NO_OFFENDER: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// 质量档
// ---------------------------------------------------------------------------

/// 质量档（与降级链 CGPU-O 的 L0..L4 同一编号）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Tier {
    /// 全质量：2× 超采样 + 三线性 + 全精度。
    L0,
    /// 4×MSAA。
    L1,
    /// 2×MSAA。
    L2,
    /// 关 AA、半精度。
    L3,
    /// 半分辨率（降级链末端）。
    L4,
}

/// 档位闭集（判据遍历用）。
pub const ALL_TIERS: [Tier; 5] = [Tier::L0, Tier::L1, Tier::L2, Tier::L3, Tier::L4];

impl Tier {
    /// 线性编号。
    pub const fn idx(self) -> usize {
        match self {
            Tier::L0 => 0,
            Tier::L1 => 1,
            Tier::L2 => 2,
            Tier::L3 => 3,
            Tier::L4 => 4,
        }
    }

    /// 编号还原（表外拒绝）。
    pub fn from_idx(i: usize) -> Result<Tier, u16> {
        if i < ALL_TIERS.len() {
            Ok(ALL_TIERS[i])
        } else {
            Err(E_Q_TIER)
        }
    }

    /// 稳定标识串。
    pub const fn name(self) -> &'static str {
        match self {
            Tier::L0 => "L0",
            Tier::L1 => "L1",
            Tier::L2 => "L2",
            Tier::L3 => "L3",
            Tier::L4 => "L4",
        }
    }
}

/// 抗锯齿模式（语义与 GPU 侧同名参数一致）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Aa {
    None,
    Msaa2,
    Msaa4,
    Ssaa2,
}

impl Aa {
    /// 每像素采样数（GPU 侧的同名字段就是它）。
    pub const fn samples(self) -> u32 {
        match self {
            Aa::None => 1,
            Aa::Msaa2 => 2,
            Aa::Msaa4 => 4,
            Aa::Ssaa2 => 4,
        }
    }

    /// 是否为超采样（超采样在**着色阶段**就放大，MSAA 只放大覆盖）。
    pub const fn is_supersample(self) -> bool {
        matches!(self, Aa::Ssaa2)
    }

    /// 采样成本权重 = 着色次数 + 覆盖样本数。
    ///
    /// MSAA 每像素只着色一次、多出来的只是覆盖样本；SSAA 的每个样本都要
    /// 真着色，所以成本是「样本数 + 样本数」。这个区分不是抠细节——它正是
    /// L0（SSAA2）质量高于 L1（MSAA4）的量化依据，缺了它两档的分数会撞在
    /// 一起，「档序越靠后质量越低」就退化成一句口号。
    pub const fn cost(self) -> u64 {
        match self {
            Aa::None => 2,
            Aa::Msaa2 => 3,
            Aa::Msaa4 => 5,
            Aa::Ssaa2 => 8,
        }
    }
}

/// 过滤模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Filter {
    Nearest,
    Bilinear,
    Trilinear,
    Aniso4,
}

impl Filter {
    /// 每像素纹理抽头数（三线性 = 2 层双线性）。
    pub const fn taps(self) -> u32 {
        match self {
            Filter::Nearest => 1,
            Filter::Bilinear => 4,
            Filter::Trilinear => 8,
            Filter::Aniso4 => 32,
        }
    }
}

/// 内部精度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Precision {
    /// 全精度（与 GPU 默认一致）。
    F32,
    /// 半精度（移动端 GPU 常见；CPU 侧仅为语义对齐）。
    F16,
}

impl Precision {
    /// 有效尾数位。
    pub const fn mantissa_bits(self) -> u32 {
        match self {
            Precision::F32 => 23,
            Precision::F16 => 10,
        }
    }
}

/// 一个档位的完整参数组合。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Profile {
    pub tier: Tier,
    pub aa: Aa,
    pub filter: Filter,
    pub precision: Precision,
    /// 渲染尺度百分比（100 = 原生，50 = 半分辨率）。
    pub scale_pct: u32,
}

impl Profile {
    /// **唯一**构造入口：档位 → 参数。任何旁路构造都会让「同语义」无法成立。
    pub const fn for_tier(t: Tier) -> Profile {
        match t {
            Tier::L0 => Profile {
                tier: Tier::L0,
                aa: Aa::Ssaa2,
                filter: Filter::Trilinear,
                precision: Precision::F32,
                scale_pct: 100,
            },
            Tier::L1 => Profile {
                tier: Tier::L1,
                aa: Aa::Msaa4,
                filter: Filter::Trilinear,
                precision: Precision::F32,
                scale_pct: 100,
            },
            Tier::L2 => Profile {
                tier: Tier::L2,
                aa: Aa::Msaa2,
                filter: Filter::Bilinear,
                precision: Precision::F32,
                scale_pct: 100,
            },
            Tier::L3 => Profile {
                tier: Tier::L3,
                aa: Aa::None,
                filter: Filter::Bilinear,
                precision: Precision::F16,
                scale_pct: 100,
            },
            Tier::L4 => Profile {
                tier: Tier::L4,
                aa: Aa::None,
                filter: Filter::Bilinear,
                precision: Precision::F16,
                scale_pct: 50,
            },
        }
    }

    /// 质量分（越大越好）：采样数 × 抽头数，再按精度与尺度打折。
    ///
    /// 「同语义」的量化表达：档位序越靠后，分数必须越低。它不是给用户看的
    /// 指标，而是让「L2 的参数组合恰好比 L1 好」这种事故在判据里直接暴露。
    pub const fn score(self) -> u64 {
        let mut s = self.aa.cost() * self.filter.taps() as u64;
        s = s * (self.precision.mantissa_bits() as u64);
        s = s * self.scale_pct as u64;
        s
    }

    /// 容差上限（单通道 8bit 差值）——**由参数推导**，不查表。
    ///
    /// 每一项都对应一个物理来源：关 AA 多 2 级（边缘锯齿）、Nearest 多 2 级
    /// （采样点漂移）、半精度多 1 级（尾数少 13 位）、降分辨率多 1 级（重建
    /// 滤波）。改档位参数而忘了改容差的事故从此不可能发生——容差跟着参数走。
    pub const fn tolerance(self) -> u8 {
        let mut t = 1u32;
        t += match self.aa {
            Aa::Ssaa2 | Aa::Msaa4 => 0,
            Aa::Msaa2 => 1,
            Aa::None => 2,
        };
        t += match self.filter {
            Filter::Aniso4 | Filter::Trilinear => 0,
            Filter::Bilinear => 1,
            Filter::Nearest => 2,
        };
        t += match self.precision {
            Precision::F32 => 0,
            Precision::F16 => 1,
        };
        t += if self.scale_pct < 100 { 1 } else { 0 };
        if t > 8 {
            t = 8;
        }
        t as u8
    }
}

// ---------------------------------------------------------------------------
// 对拍
// ---------------------------------------------------------------------------

/// 一次对拍的结果（入册的最小单位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiffReport {
    /// 比较的像素数。
    pub px: u32,
    /// 超阈像素数。
    pub over: u32,
    /// 最大单通道差值。
    pub max_diff: u8,
    /// 逐像素最大通道差之和（均值分子）。
    pub sum_diff: u64,
    /// 首个超阈像素的线性序号（无则 `NO_OFFENDER`）。
    pub first_over: u32,
    /// 本次比较使用的容差。
    pub tol: u8,
}

impl DiffReport {
    /// 平均逐像素最大通道差（千分比，四舍五入）。
    pub fn mean_milli(&self) -> u32 {
        if self.px == 0 {
            return 0;
        }
        let num = self.sum_diff * 1000 + (self.px as u64 / 2);
        (num / self.px as u64) as u32
    }

    /// 是否通过（无任何超阈像素）。
    pub fn passed(&self) -> bool {
        self.over == 0
    }
}

/// 逐像素逐通道对拍。
///
/// `cpu`/`gpu` 均为紧排 RGBA8。逐像素取「四个通道里最大的差」作为该像素的
/// 差——不用通道平均：一个通道偏 8、其余三个恰好抵消，平均下来是 2，而肉眼
/// 看到的是一条偏色边。**最大值才是人眼判据**。
pub fn compare(
    cpu: &[u8],
    gpu: &[u8],
    w: usize,
    h: usize,
    p: Profile,
) -> Result<DiffReport, u16> {
    if w == 0 || h == 0 {
        return Err(E_Q_DIM);
    }
    let need = w * h * 4;
    if cpu.len() < need || gpu.len() < need {
        return Err(E_Q_SHORT);
    }
    let tol = p.tolerance();
    let mut r = DiffReport {
        px: (w * h) as u32,
        over: 0,
        max_diff: 0,
        sum_diff: 0,
        first_over: NO_OFFENDER,
        tol,
    };
    let mut i = 0usize;
    while i < need {
        let mut d = 0u8;
        let mut c = 0usize;
        while c < 4 {
            let a = cpu[i + c];
            let b = gpu[i + c];
            let v = if a > b { a - b } else { b - a };
            if v > d {
                d = v;
            }
            c += 1;
        }
        if d > r.max_diff {
            r.max_diff = d;
        }
        r.sum_diff += d as u64;
        if d > tol {
            if r.over == 0 {
                r.first_over = (i / 4) as u32;
            }
            r.over += 1;
        }
        i += 4;
    }
    Ok(r)
}

// ---------------------------------------------------------------------------
// 入册
// ---------------------------------------------------------------------------

/// 一行对拍账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerRow {
    pub tier: Tier,
    pub mode: BlendMode,
    pub over: u32,
    pub max_diff: u8,
    pub mean_milli: u32,
    pub tol: u8,
    pub passed: bool,
}

/// 滚动对拍账（定容，超容丢最旧——对拍是持续行为，不该无界增长）。
pub struct DiffLedger {
    rows: Vec<LedgerRow>,
    cap: usize,
    total: u32,
    failed: u32,
}

impl DiffLedger {
    /// 新建（`cap == 0` 时只计数不入册）。
    pub fn new(cap: usize) -> DiffLedger {
        DiffLedger {
            rows: Vec::with_capacity(cap),
            cap,
            total: 0,
            failed: 0,
        }
    }

    /// 记账。返回值指出这一行是否通过（调用方不必回查账本）。
    pub fn record(&mut self, tier: Tier, mode: BlendMode, r: &DiffReport) -> bool {
        self.total = self.total.saturating_add(1);
        let passed = r.passed();
        if !passed {
            self.failed = self.failed.saturating_add(1);
        }
        if self.cap > 0 {
            if self.rows.len() >= self.cap {
                self.rows.remove(0);
            }
            self.rows.push(LedgerRow {
                tier,
                mode,
                over: r.over,
                max_diff: r.max_diff,
                mean_milli: r.mean_milli(),
                tol: r.tol,
                passed,
            });
        }
        passed
    }

    /// 入册行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 空账。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 累计次数（含已滚出的）。
    pub fn total(&self) -> u32 {
        self.total
    }

    /// 累计失败次数。
    pub fn failed(&self) -> u32 {
        self.failed
    }

    /// 通过率千分比（空账返回 0——空集上的「全通过」是假信号）。
    pub fn pass_permille(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.total - self.failed) as u32 * 1000 / self.total
    }

    /// 红项名单：**(档位, 模式, 超阈数, 容差)**，按入册序。
    ///
    /// 返回结构化名单而不是「N 红」——聚合成一个数字后，定位要靠二分重跑，
    /// 那是分钟级成本；名单直接把靶子贴出来。
    pub fn red_rows(&self) -> Vec<(Tier, BlendMode, u32, u8)> {
        let mut v = Vec::new();
        for r in self.rows.iter() {
            if !r.passed {
                v.push((r.tier, r.mode, r.over, r.tol));
            }
        }
        v
    }

    /// 清账（保留累计计数，便于「换场景不清总账」）。
    pub fn clear_rows(&mut self) {
        self.rows.clear();
    }
}

// ---------------------------------------------------------------------------
// 全混合模式覆盖
// ---------------------------------------------------------------------------

/// 24 式混合模式覆盖位图。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModeCoverage {
    bits: u32,
}

impl ModeCoverage {
    /// 空覆盖。
    pub const fn new() -> ModeCoverage {
        ModeCoverage { bits: 0 }
    }

    /// 标记一个模式已对拍。
    pub fn mark(&mut self, m: BlendMode) {
        let c = m.code();
        if (c as usize) < MODE_COUNT {
            self.bits |= 1u32 << c;
        }
    }

    /// 按编号标记（越界拒绝——调用方拿到的模式编号可能来自遥测/配置）。
    pub fn mark_checked(&mut self, code: u16) -> Result<(), u16> {
        if (code as usize) >= MODE_COUNT {
            return Err(E_Q_MODE);
        }
        self.bits |= 1u32 << code;
        Ok(())
    }

    /// 由账本重建覆盖（只认**通过**的行：红行意味着该模式还没达标）。
    pub fn from_ledger(l: &DiffLedger) -> ModeCoverage {
        let mut c = ModeCoverage::new();
        for r in l.rows.iter() {
            if r.passed {
                c.mark(r.mode);
            }
        }
        c
    }

    /// 已覆盖模式数。
    pub fn count(&self) -> u32 {
        let mut n = 0u32;
        let mut b = self.bits;
        while b != 0 {
            n += b & 1;
            b >>= 1;
        }
        n
    }

    /// 是否全覆盖。
    pub fn complete(&self) -> bool {
        self.count() as usize == MODE_COUNT
    }

    /// 未覆盖的模式（按编号序）。
    pub fn missing(&self) -> Vec<BlendMode> {
        let mut v = Vec::new();
        let mut i = 0usize;
        while i < MODE_COUNT {
            if self.bits & (1u32 << i) == 0 {
                v.push(BlendMode::ALL[i]);
            }
            i += 1;
        }
        v
    }
}

impl Default for ModeCoverage {
    fn default() -> ModeCoverage {
        ModeCoverage::new()
    }
}

// ---------------------------------------------------------------------------
// 回归门
// ---------------------------------------------------------------------------

/// 门禁结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GateResult {
    /// 是否放行。
    pub pass: bool,
    /// 红项名单（档位, 模式, 超阈数）。
    pub red: Vec<(Tier, BlendMode, u32)>,
    /// 未覆盖的模式数。
    pub uncovered: u32,
    /// 缺的档位（一个都没对拍过的档）。
    pub missing_tiers: Vec<Tier>,
}

/// 回归门：**全档位无红 且 24 式全覆盖** 才放行。
///
/// 为什么把覆盖也算进门：一个从未被对拍的模式在账本里与「通过」长得一模一样
/// ——两者都贡献零个红项。若门只看红项数，把某模式的对拍代码整段删掉反而
/// 让门更容易通过，这是最该被堵死的反向激励。
pub fn regression_gate(l: &DiffLedger) -> GateResult {
    let red_v = l.red_rows();
    let mut red = Vec::with_capacity(red_v.len());
    for (t, m, o, _tol) in red_v.iter() {
        red.push((*t, *m, *o));
    }
    let cov = ModeCoverage::from_ledger(l);
    let uncovered = (MODE_COUNT as u32) - cov.count();
    // 每档至少要有一行通过记录，否则该档的「无红」只是没测过。
    let mut missing_tiers = Vec::new();
    for t in ALL_TIERS.iter() {
        let mut seen = false;
        for r in l.rows.iter() {
            if r.tier == *t && r.passed {
                seen = true;
            }
        }
        if !seen {
            missing_tiers.push(*t);
        }
    }
    GateResult {
        pass: red.is_empty() && uncovered == 0 && missing_tiers.is_empty(),
        red,
        uncovered,
        missing_tiers,
    }
}

// ---------------------------------------------------------------------------
// 降级联动
// ---------------------------------------------------------------------------

/// 压力（千分比）→ 档位。值越大档位越低，且必须单调。
pub fn tier_for_pressure(pressure_permille: u32) -> Tier {
    if pressure_permille < 250 {
        Tier::L0
    } else if pressure_permille < 450 {
        Tier::L1
    } else if pressure_permille < 650 {
        Tier::L2
    } else if pressure_permille < 850 {
        Tier::L3
    } else {
        Tier::L4
    }
}

/// 按档位缩放后的渲染尺寸（向上取整——向下取整会把 1 像素宽的线整条丢光）。
pub fn render_extent(w: usize, h: usize, p: Profile) -> (usize, usize) {
    let s = p.scale_pct as usize;
    let sw = (w * s + 99) / 100;
    let sh = (h * s + 99) / 100;
    (sw.max(1), sh.max(1))
}

/// 门禁的 Result 形态（给只想「过/不过」的调用方；红项细节走 `regression_gate`）。
pub fn regression_gate_strict(l: &DiffLedger) -> Result<(), u16> {
    if regression_gate(l).pass {
        Ok(())
    } else {
        Err(E_Q_GATE)
    }
}

/// run_cga23_checks：CGPU-F0023 判据聚合（0xD0 段）。
pub fn run_cga23_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0023 质量档与对拍");

    // 正-01：档位编号双射 + 五档全在闭集
    {
        let mut ok = true;
        for (i, t) in ALL_TIERS.iter().enumerate() {
            if t.idx() != i || Tier::from_idx(i) != Ok(*t) {
                ok = false;
            }
            if t.name().is_empty() {
                ok = false;
            }
        }
        if ok && Tier::from_idx(ALL_TIERS.len()).is_err() {
            s.ok("F0023-正-01-档位编号双射");
        } else {
            s.fail("F0023-正-01-档位编号双射", "档位编号或闭集错位");
        }
    }

    // 正-02：质量档同语义——档序越靠后质量分必须严格越低，且 scale 只在 L4 破 100
    {
        let mut ok = true;
        let mut prev = u64::MAX;
        for t in ALL_TIERS.iter() {
            let p = Profile::for_tier(*t);
            if p.tier != *t {
                ok = false;
            }
            let sc = p.score();
            if sc >= prev {
                ok = false;
            }
            prev = sc;
            let expect_scale = if *t == Tier::L4 { 50 } else { 100 };
            if p.scale_pct != expect_scale {
                ok = false;
            }
        }
        if ok {
            s.ok("F0023-正-02-质量档同语义");
        } else {
            s.fail("F0023-正-02-质量档同语义", "档序质量分非严格递减或尺度错");
        }
    }

    // 正-03：降级联动——L4 是半分辨率，且奇数尺寸向上取整
    {
        let p4 = Profile::for_tier(Tier::L4);
        let (w1, h1) = render_extent(4, 4, p4);
        let (w2, h2) = render_extent(5, 5, p4);
        let (w3, h3) = render_extent(1, 1, p4);
        let p0 = Profile::for_tier(Tier::L0);
        let (w4, h4) = render_extent(5, 3, p0);
        let ok = (w1, h1) == (2, 2)
            && (w2, h2) == (3, 3)
            && (w3, h3) == (1, 1)
            && (w4, h4) == (5, 3);
        if ok {
            s.ok("F0023-正-03-降级联动半分辨率");
        } else {
            s.fail("F0023-正-03-降级联动半分辨率", "L4 尺度或取整不符");
        }
    }

    // 正-04：压力→档位必须单调（压力越大档位越低，不许回跳）
    {
        let mut ok = true;
        let mut prev = Tier::L0;
        let mut p = 0u32;
        while p <= 1000 {
            let t = tier_for_pressure(p);
            if t.idx() < prev.idx() {
                ok = false;
            }
            prev = t;
            p += 25;
        }
        if ok && tier_for_pressure(0) == Tier::L0 && tier_for_pressure(1000) == Tier::L4 {
            s.ok("F0023-正-04-压力档位单调");
        } else {
            s.fail("F0023-正-04-压力档位单调", "压力映射非单调或端点错");
        }
    }

    // 正-05：容差随档位收紧（L0 最严、L4 最松），且由参数推导而非查表
    {
        let mut ok = true;
        let mut prev = 0u8;
        for t in ALL_TIERS.iter() {
            let tol = Profile::for_tier(*t).tolerance();
            if tol < prev || tol == 0 {
                ok = false;
            }
            prev = tol;
            if tol > 8 {
                ok = false;
            }
        }
        // 参数化推导的自证：改一个参数，容差必须跟着动
        let mut p = Profile::for_tier(Tier::L2);
        let base = p.tolerance();
        p.aa = Aa::None;
        if p.tolerance() <= base {
            ok = false;
        }
        p.filter = Filter::Nearest;
        p.precision = Precision::F16;
        p.scale_pct = 50;
        if p.tolerance() != 7 {
            ok = false;
        }
        if ok {
            s.ok("F0023-正-05-容差随参数推导");
        } else {
            s.fail("F0023-正-05-容差随参数推导", "容差非单调或不随参数变化");
        }
    }

    // 正-06：完全相同 → 零差异通过；且 first_over 为空哨兵
    {
        let px: Vec<u8> = (0..(8 * 8 * 4)).map(|i| (i * 7) as u8).collect();
        let r = compare(&px, &px, 8, 8, Profile::for_tier(Tier::L0)).expect("compare");
        if r.passed() && r.over == 0 && r.max_diff == 0 && r.first_over == NO_OFFENDER
            && r.mean_milli() == 0
            && r.px == 64
        {
            s.ok("F0023-正-06-零差异通过");
        } else {
            s.fail("F0023-正-06-零差异通过", "同图对拍未给出零差异");
        }
    }

    // 正-07：单点超标必须被精确点到（定位精度 = 像素序号）
    {
        let mut a = alloc::vec![0u8; 8 * 8 * 4];
        let mut b = alloc::vec![0u8; 8 * 8 * 4];
        // 在 (x=5,y=3) 的 G 通道塞 +9
        let idx = (3 * 8 + 5) * 4 + 1;
        b[idx] = 9;
        a[idx] = 0;
        let r = compare(&a, &b, 8, 8, Profile::for_tier(Tier::L0)).expect("compare");
        if r.over == 1 && r.max_diff == 9 && r.first_over == (3 * 8 + 5) as u32 && !r.passed() {
            s.ok("F0023-正-07-单点超标精确定位");
        } else {
            s.fail("F0023-正-07-单点超标精确定位", "超标点定位错");
        }
    }

    // 正-08：±2 交替噪声——均值接近 0 但必须判红（防「平均下来没事」）
    {
        let n = 16 * 16 * 4;
        let mut a = alloc::vec![100u8; n];
        let mut b = alloc::vec![100u8; n];
        let mut i = 0usize;
        while i < n {
            // 偶数像素 +2，奇数像素 −2：整图均值恰为 0
            let p = i / 4;
            b[i] = if p % 2 == 0 { 102 } else { 98 };
            a[i] = 100;
            i += 1;
        }
        // 带符号均值在判据侧独立算：正负恰好抵消 → 0
        let mut signed: i64 = 0;
        for i in 0..n {
            signed += b[i] as i64 - a[i] as i64;
        }
        let r = compare(&a, &b, 16, 16, Profile::for_tier(Tier::L0)).expect("compare");
        // mean_milli 是「平均绝对差」，对称噪声下它等于 2000‰（每像素差 2）。
        let ok = signed == 0
            && r.mean_milli() == 2_000
            && r.max_diff == 2
            && r.over == 256
            && !r.passed();
        if ok {
            s.ok("F0023-正-08-带符号归零仍判红");
        } else {
            s.fail("F0023-正-08-带符号归零仍判红", "对称噪声被均值掩盖");
        }
    }

    // 正-09：容差必须真的被用上——差 2 在 L0（容差 1）红、在 L4（容差 6）绿
    {
        let n = 4 * 4 * 4;
        let a = alloc::vec![50u8; n];
        let b = alloc::vec![52u8; n];
        let l0 = compare(&a, &b, 4, 4, Profile::for_tier(Tier::L0)).expect("l0");
        let l4 = compare(&a, &b, 4, 4, Profile::for_tier(Tier::L4)).expect("l4");
        let ok = !l0.passed() && l0.tol == 1 && l4.passed() && l4.tol == 6 && l4.max_diff == 2;
        if ok {
            s.ok("F0023-正-09-容差分级生效");
        } else {
            s.fail("F0023-正-09-容差分级生效", "两组容差未区分同一差异");
        }
    }

    // 正-10：入册——滚动定容、通过率、红项名单三元组
    {
        let mut l = DiffLedger::new(3);
        let ok_r = compare(&[0u8; 16], &[0u8; 16], 2, 2, Profile::for_tier(Tier::L0)).expect("ok");
        let bad = compare(&[0u8; 16], &[40u8; 16], 2, 2, Profile::for_tier(Tier::L0)).expect("bad");
        l.record(Tier::L0, BlendMode::Normal, &ok_r);
        l.record(Tier::L1, BlendMode::Multiply, &bad);
        l.record(Tier::L2, BlendMode::Screen, &ok_r);
        l.record(Tier::L3, BlendMode::Overlay, &ok_r); // 挤掉第一行
        let red = l.red_rows();
        let ok = l.len() == 3
            && l.total() == 4
            && l.failed() == 1
            && l.pass_permille() == 750
            && red.len() == 1
            && red[0].0 == Tier::L1
            && red[0].1 == BlendMode::Multiply;
        // 空账的通过率必须是 0，不是 1000
        let empty = DiffLedger::new(4);
        if ok && empty.pass_permille() == 0 && empty.is_empty() {
            s.ok("F0023-正-10-对拍入册与红项名单");
        } else {
            s.fail("F0023-正-10-对拍入册与红项名单", "账本计数或红项名单不符");
        }
    }

    // 正-11：全混合模式覆盖——23 式不算覆盖，24 式齐全才算
    {
        let mut cov = ModeCoverage::new();
        let mut i = 0usize;
        while i < MODE_COUNT - 1 {
            cov.mark(BlendMode::ALL[i]);
            i += 1;
        }
        let partial_ok = cov.count() as usize == MODE_COUNT - 1
            && !cov.complete()
            && cov.missing().len() == 1
            && cov.missing()[0] == BlendMode::ALL[MODE_COUNT - 1];
        cov.mark(BlendMode::ALL[MODE_COUNT - 1]);
        // 重复标记不得让计数虚增
        cov.mark(BlendMode::Normal);
        if partial_ok && cov.complete() && cov.count() as usize == MODE_COUNT && cov.missing().is_empty()
        {
            s.ok("F0023-正-11-全混合模式覆盖");
        } else {
            s.fail("F0023-正-11-全混合模式覆盖", "覆盖计数或缺失名单不符");
        }
    }

    // 正-12：回归门——红项、未覆盖、缺档三种拦截各自独立生效
    {
        let ok_r = compare(&[0u8; 16], &[0u8; 16], 2, 2, Profile::for_tier(Tier::L0)).expect("ok");
        let bad = compare(&[0u8; 16], &[99u8; 16], 2, 2, Profile::for_tier(Tier::L0)).expect("bad");

        // (a) 全绿但覆盖不全 → 拒
        let mut l1 = DiffLedger::new(64);
        l1.record(Tier::L0, BlendMode::Normal, &ok_r);
        let g1 = regression_gate(&l1);

        // (b) 24 式全覆盖且全绿 → 放行（五档都要有通过记录）
        let mut l2 = DiffLedger::new(256);
        for m in BlendMode::ALL.iter() {
            for t in ALL_TIERS.iter() {
                l2.record(*t, *m, &ok_r);
            }
        }
        let g2 = regression_gate(&l2);
        l2.record(Tier::L3, BlendMode::Hue, &bad);
        let g3 = regression_gate(&l2);

        let ok = !g1.pass
            && g1.uncovered == (MODE_COUNT as u32 - 1)
            && g1.red.is_empty()
            && g1.missing_tiers.len() == ALL_TIERS.len() - 1
            && g2.pass
            && g2.red.is_empty()
            && g2.uncovered == 0
            && g2.missing_tiers.is_empty()
            && !g3.pass
            && g3.red.len() == 1
            && g3.red[0].0 == Tier::L3
            && g3.red[0].1 == BlendMode::Hue;
        if ok {
            s.ok("F0023-正-12-回归门三种拦截");
        } else {
            s.fail("F0023-正-12-回归门三种拦截", "门禁未按红项/覆盖/缺档拦截");
        }
    }

    // 正-13：只认「通过」的行计入覆盖——红行不得冒充已覆盖
    {
        let bad = compare(&[0u8; 16], &[99u8; 16], 2, 2, Profile::for_tier(Tier::L0)).expect("bad");
        let mut l = DiffLedger::new(8);
        l.record(Tier::L0, BlendMode::Normal, &bad);
        let cov = ModeCoverage::from_ledger(&l);
        if cov.count() == 0 && !cov.complete() {
            s.ok("F0023-正-13-红行不计覆盖");
        } else {
            s.fail("F0023-正-13-红行不计覆盖", "红行被算成已覆盖");
        }
    }

    // 正-14：尺寸/缓冲/空帧拒绝路径码位分离
    {
        let a = compare(&[], &[], 0, 4, Profile::for_tier(Tier::L0));
        let b = compare(&[0u8; 8], &[0u8; 64], 4, 4, Profile::for_tier(Tier::L0));
        let c = compare(&[0u8; 64], &[0u8; 8], 4, 4, Profile::for_tier(Tier::L0));
        let ok = a == Err(E_Q_DIM) && b == Err(E_Q_SHORT) && c == Err(E_Q_SHORT);
        if ok {
            s.ok("F0023-正-14-拒绝路径码位分离");
        } else {
            s.fail("F0023-正-14-拒绝路径码位分离", "拒绝码混用或漏检");
        }
    }

    // 编-01：确定性——同输入同报告（含 first_over 与 mean_milli）
    {
        let n = 32 * 32 * 4;
        let mut a = alloc::vec![0u8; n];
        let mut b = alloc::vec![0u8; n];
        let mut state = 0x5EED_1234u32;
        for i in 0..n {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            a[i] = (state >> 24) as u8;
            b[i] = ((state >> 16) & 0xFC) as u8;
        }
        let p = Profile::for_tier(Tier::L2);
        let r1 = compare(&a, &b, 32, 32, p).expect("r1");
        let r2 = compare(&a, &b, 32, 32, p).expect("r2");
        if r1 == r2 {
            s.ok("F0023-编-01-对拍确定性");
        } else {
            s.fail("F0023-编-01-对拍确定性", "同输入两次报告不同");
        }
    }

    // 编-02：帐本 clear_rows 保留累计计数（换场景不清总账）
    {
        let ok_r = compare(&[0u8; 16], &[0u8; 16], 2, 2, Profile::for_tier(Tier::L0)).expect("ok");
        let mut l = DiffLedger::new(4);
        l.record(Tier::L0, BlendMode::Normal, &ok_r);
        l.record(Tier::L1, BlendMode::Normal, &ok_r);
        l.clear_rows();
        if l.is_empty() && l.total() == 2 && l.failed() == 0 && l.pass_permille() == 1000 {
            s.ok("F0023-编-02-清册保留总账");
        } else {
            s.fail("F0023-编-02-清册保留总账", "清册把累计计数一起清了");
        }
    }

    // 编-03：越界模式编号必须拒绝；门禁 Result 形态与结构形态结论一致
    {
        let mut cov = ModeCoverage::new();
        let bad = cov.mark_checked(MODE_COUNT as u16);
        let good = cov.mark_checked(0);
        let ok_r = compare(&[0u8; 16], &[0u8; 16], 2, 2, Profile::for_tier(Tier::L0)).expect("ok");
        let mut l = DiffLedger::new(4);
        l.record(Tier::L0, BlendMode::Normal, &ok_r);
        let strict = regression_gate_strict(&l);
        let allow = regression_gate_strict(&{
            let mut l2 = DiffLedger::new(256);
            for m in BlendMode::ALL.iter() {
                for t in ALL_TIERS.iter() {
                    l2.record(*t, *m, &ok_r);
                }
            }
            l2
        });
        if bad == Err(E_Q_MODE) && good.is_ok() && cov.count() == 1 && strict == Err(E_Q_GATE) && allow.is_ok()
        {
            s.ok("F0023-编-03-越界模式与门禁形态");
        } else {
            s.fail("F0023-编-03-越界模式与门禁形态", "越界标记或门禁形态不符");
        }
    }

    s
}
