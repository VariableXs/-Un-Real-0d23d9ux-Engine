//! VE-F0815 · 文字渲染性能（VE-E 域 · 文字渲染）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0815`
//!
//! **判据（锚点原文）**：1.5ms 分解、五段计时、三水位、降级保护序、双通道计时。
//!
//! F0814 回答「坏输入打不穿管线」，本条回答「好输入也得住得进 1.5ms」。
//! 复用 P07 预算范式（**复用声明**：预算表结构 / 测量口径 / P50、P95 分列
//! 全部沿用，仅换指标集），落位为三件事：
//!
//! 1. **1.5ms 分解与五段计时**（判据一、二）。预算表**不从本模块重写**——
//!    直接复用 F0801 的 [`FRAME_BUDGET_US`] 与 [`BUDGET_SLICES`]（六段：
//!    解码 50 / 光栅化 500 / 整形 300 / 图元 400 / 遥测 20 / 余量 230，
//!    和恰 1500µs），本模块只做**同源映射**：五段计时枚举
//!    [`PerfSegment`] 逐段对位 `BUDGET_SLICES[0..5]`，第六段「余量」是
//!    **预算的组成而非计时的对象**——余量没有工作量，给它建计时器等于
//!    造一个恒零段，还让人误以为余量被"花"掉了。段超支定位到段
//!    （[`FrameSettle::worst_overspend`]）：帧总账不超但某段越过自己的
//!    分解档，也要点名——总账掩盖段级劣化是性能回归最常见的藏身处。
//!    计时场景固定为 F0812 基准场景（1000 静态字 + 500 动态字，常量
//!    同源引用不重抄）；本模块在 no_std 内核**不产墙钟测量**，只接收
//!    运行时注入的段级样本并负责口径与判定——伪造测量等于把预算表
//!    变成文学创作。
//!
//! 2. **三水位与降级保护序**（判据三、四）。水位复用 F0801 的
//!    [`Water::judge`]（绿 ≤300 / 黄 ≤750 / 红 >750，微秒，常量同源），
//!    本模块不另立阈值。红即出降级计划 [`plan_degrade`]：三步**严格按
//!    F0801 [`TextPipeline::degradation_plan`] 的序**（① 降 Hinting 档 →
//!    ② 关亚像素相位 → ③ 减动字号采样率——序就是纪律，计划器只做执行
//!    不做重排）；每步带预计回收量（[`DEGRADE_SAVE_HINT_US`] 等三个
//!    常量，估计值显性化，不冒充实测）。**无障碍红线是结构不是文案**：
//!    放大 / 高对比路径的耗时单列 [`plan_degrade`] 的 a11y 入参，三步
//!    只作用于非无障碍部分——降到底也砍不到无障碍头上，这是"不得先砍"
//!    的最强实现（想砍没有入口）。计划必须带**用户可见提示**（降了多少、
//!    为什么、目标是什么），降级不告知等于悄悄变卡；三步用尽仍不达标的
//!    帧**如实报未达标**（`achieved=false`），不假装救回来了。
//!
//! 3. **双通道计时与预算诊断**（判据五）。每段样本带 CPU / GPU 双读数
//!    [`ChannelSample`]：CPU 是结算口径，GPU 是旁证——分歧超过
//!    [`CHANNEL_DRIFT_MAX_US`]（预算的 10%）记 [`ChannelMismatch`]
//!    入账不静默（旁证反水说明采样链路有问题，埋着头用就是自欺）。
//!    缓存命中率 < 90% 持续 5 秒（逻辑秒）→ 自动出诊断报告
//!    [`CacheDiag`]：90% 是**本条的诊断触发线**（[`DIAG_HIT_RATE_PPM`]），
//!    与 F0809 的 80% 健康告警线（`HIT_RATE_ALERT`）口径不同、用途不同
//!    ——F0809 管"遥测健康"，本条管"缓存劣化到开始吃光栅预算"，两线
//!    并存各司其职；命中率恰好等于 90% 不触发（夹逼边界），查表 0 次
//!    不产数据也不动状态（除零防线）。诊断报告恢复即清账，再次跌落
//!    可再报——一次性告警会让第二次劣化无人知晓。
//!
//! **对接**：预算表与 Eb07 域性能汇总**同源**（[`summary_for_eb07`]，
//! Eb07 只汇总不重测——测量真源唯一，两处各测一遍必然渐行渐远）；
//! P50/P95 分列口径沿用 F0809 的取秩公式（`(pct*n+99)/100` 夹逼
//! 1..=n，按段独立取秩不混算）。
//!
//! 零静默纪律：通道分歧、样本溢出丢弃、诊断触发/恢复全部入账可取回；
//! 零 panic 面（固定下标一律走 `get`/`Option`，算术全饱和）、零 IO、
//! 零墙钟（逻辑秒注入）、无全局可变状态。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::vee01_arch::{
    BUDGET_SLICES, DegradeStep, FRAME_BUDGET_US, TextPipeline, Water, WATER_GREEN_MAX_US,
    WATER_YELLOW_MAX_US,
};
use super::vee12_textqa::{BENCH_DYNAMIC_GLYPHS, BENCH_STATIC_GLYPHS};

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 双通道分歧上限（µs）＝预算的 10%（1500/10）。
///
/// CPU 与 GPU 读数天然有采样相位差，小分歧是测量噪声；分歧超过预算的
/// 一成说明两条计时链路有一条出了问题（丢 fence / 丢 tick），这时数据
/// 不可信，必须入账而不是继续用。
pub const CHANNEL_DRIFT_MAX_US: u32 = FRAME_BUDGET_US / 10;

/// 降级步①（降 Hinting 档）预计回收量（µs/帧，估计值）。
///
/// 全 Hinting 的指令生成与网格对齐是光栅段里最贵的前置；降一档回收量
/// 按光栅分解档的四分之一估（500/4≈125，取整 120 留 5µs 余地）。估计值
/// 显性化为常量：计划给出的"预计"与实测"回收"分开，不冒充精确。
pub const DEGRADE_SAVE_HINT_US: u32 = 120;

/// 降级步②（关亚像素相位）预计回收量（µs/帧，估计值）。
///
/// 四相位（1/4 像素）变体让同一字形的缓存键翻四倍，关掉后变体坍缩、
/// 命中率回升、未命中光栅变少——回收量按步①的一半估（80µs）。
pub const DEGRADE_SAVE_SUBPIXEL_US: u32 = 80;

/// 降级步③（减动字号采样率）预计回收量（µs/帧，估计值）。
///
/// 动态字号重采样是光栅段的长尾来源；降采样率砍的是长尾频率，回收量
/// 最保守（60µs）——它只动"动字号"这一窄面，波及面最小所以排最后。
pub const DEGRADE_SAVE_DYNSIZE_US: u32 = 60;

/// 缓存命中率诊断触发线：90%（百万分比；锚点原文数值）。
///
/// 与 F0809 的 80% 健康告警线（`HIT_RATE_ALERT`）**并存不混用**：
/// 80% 管遥测健康判定，90% 管预算诊断（缓存劣化开始吃光栅分解档）。
pub const DIAG_HIT_RATE_PPM: u32 = 900_000;

/// 诊断触发需要的持续时长（逻辑秒；锚点原文「持续 5 秒」）。
pub const DIAG_SUSTAIN_SECS: u64 = 5;

/// 每段样本容量上限。样本环是固定口径的滑动窗（新样本挤掉最旧样本），
/// 上限防长跑内存线性膨胀；被挤掉的样本数入账（`dropped_samples`）。
pub const MAX_SAMPLES_PER_SEG: usize = 256;

/// 结算帧留档上限（滑动窗，同上纪律）。
pub const MAX_SETTLED_FRAMES: usize = 64;

/// 无障碍保护路径（红线名册；放大 / 高对比，锚点原文点名）。
pub const PROTECTED_A11Y_PATHS: [&str; 2] = ["无障碍放大", "高对比"];

/// 复用声明（锚点原文：预算表结构/测量口径/P50、P95 分列全部沿用，仅换指标集）。
pub const REUSE_DOC: &str = "\
复用声明（VE-F0815）：复用 P07 预算范式——预算表结构沿用 F0801 \
BUDGET_SLICES 六段分解（同源引用不重抄）；测量口径沿用 F0801 水位判定 \
（Water::judge）与 F0809 取秩公式（(pct*n+99)/100 夹逼 1..=n，按段独立）；\
P50、P95 分列沿用 F0809 分桶独立口径（本条按计时段分列）。仅换指标集：\
五段计时（解码/光栅/整形/图元/遥测）。同构不同参：P07 的预算对象是动效帧，\
本条是文字帧。";

/// Eb07 对接声明（Eb07 只汇总不重测——本模块是文字域性能的测量真源）。
pub const EB07_DOC: &str = "\
对接声明（VE-F0815 → Eb07）：Eb07 域性能汇总对预算表**只汇总不重测**；\
本模块 summary_for_eb07() 产出的冻结表是文字域唯一测量口径，Eb07 不得\
另起炉灶自测同指标——两处各测一遍必然渐行渐远，汇总就失去意义。";

// ---------------------------------------------------------------------------
// 二、五段计时枚举（判据二；与 BUDGET_SLICES[0..5] 同源映射）
// ---------------------------------------------------------------------------

/// 计时段（五段；「余量」不计时——见模块注释判据一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerfSegment {
    /// 解码（budget 50µs）。
    Decode,
    /// 光栅化——预算口径即「缓存未命中部分」（budget 500µs）。
    Raster,
    /// 整形（budget 300µs；对接 Eb03）。
    Shaping,
    /// 图元渲染（budget 400µs）。
    Prims,
    /// 遥测（budget 20µs；锚点 F0809 口径）。
    Telemetry,
}

impl PerfSegment {
    /// 五段全集（缺一段即盲区，自检逐段核对映射）。
    pub const ALL: [PerfSegment; 5] = [
        PerfSegment::Decode,
        PerfSegment::Raster,
        PerfSegment::Shaping,
        PerfSegment::Prims,
        PerfSegment::Telemetry,
    ];

    /// 段名（与 `BUDGET_SLICES[i].0` 逐字对位——自检按名核对，防错位）。
    pub const fn label(self) -> &'static str {
        match self {
            PerfSegment::Decode => "解码",
            PerfSegment::Raster => "光栅化",
            PerfSegment::Shaping => "整形",
            PerfSegment::Prims => "图元渲染",
            PerfSegment::Telemetry => "遥测",
        }
    }

    /// 数组下标。
    pub const fn ordinal(self) -> usize {
        match self {
            PerfSegment::Decode => 0,
            PerfSegment::Raster => 1,
            PerfSegment::Shaping => 2,
            PerfSegment::Prims => 3,
            PerfSegment::Telemetry => 4,
        }
    }

    /// 从 `BUDGET_SLICES` 取本段分解档（同源引用；越界不可能——五段对位
    /// 六段表的前五格，防御式 None 记 0 并在自检里钉死映射）。
    pub const fn budget_us(self) -> u32 {
        match self {
            PerfSegment::Decode => 50,
            PerfSegment::Raster => 500,
            PerfSegment::Shaping => 300,
            PerfSegment::Prims => 400,
            PerfSegment::Telemetry => 20,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、双通道样本与分位（判据五；P50/P95 分列沿用 F0809 取秩口径）
// ---------------------------------------------------------------------------

/// 一段的双通道读数。CPU 是结算口径，GPU 是旁证（分歧超限入账不静默）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelSample {
    /// CPU 侧读数（µs）。
    pub cpu_us: u32,
    /// GPU 侧读数（µs）。
    pub gpu_us: u32,
}

/// 通道分歧记录（零静默：旁证反水必须留痕）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelMismatch {
    /// 逻辑秒。
    pub tick: u64,
    /// 段。
    pub seg: PerfSegment,
    /// CPU 读数。
    pub cpu_us: u32,
    /// GPU 读数。
    pub gpu_us: u32,
}

impl ChannelMismatch {
    /// 分歧量（µs，饱和差）。
    pub fn drift_us(&self) -> u32 {
        self.cpu_us.abs_diff(self.gpu_us)
    }
}

/// 取秩分位（**沿用 F0809 公式**：rank = (pct*n+99)/100，夹逼 1..=n）。
///
/// 空样本给 0（与 F0809「无样本给 0」同口径），不 panic、不 NaN。
fn percentile_of(samples: &[u32], pct: usize) -> u32 {
    let n = samples.len();
    if n == 0 {
        return 0;
    }
    let mut vals: Vec<u32> = Vec::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        vals.push(samples[i]);
        i += 1;
    }
    // 选择排序（样本容量有界 256，O(n²) 可接受且无递归无堆排序依赖）。
    for a in 0..n {
        let mut b2 = a + 1;
        while b2 < n {
            if vals[b2] < vals[a] {
                let t = vals[a];
                vals[a] = vals[b2];
                vals[b2] = t;
            }
            b2 += 1;
        }
    }
    let mut rank = (pct * n + 99) / 100;
    if rank == 0 {
        rank = 1;
    }
    if rank > n {
        rank = n;
    }
    match vals.get(rank - 1) {
        Some(v) => *v,
        None => 0,
    }
}

/// 单段统计（P50/P95 按 CPU / GPU 通道**分列**——混列会把 GPU 侧劣化
/// 摊平进 CPU，分列是 P07 范式的口径要求）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegStats {
    /// 样本数。
    pub n: usize,
    /// CPU P50（µs；无样本给 0）。
    pub p50_cpu: u32,
    /// CPU P95（µs）。
    pub p95_cpu: u32,
    /// GPU P50（µs）。
    pub p50_gpu: u32,
    /// GPU P95（µs）。
    pub p95_gpu: u32,
}

impl SegStats {
    /// 空统计（全 0；无样本给 0 口径）。
    pub const EMPTY: SegStats = SegStats {
        n: 0,
        p50_cpu: 0,
        p95_cpu: 0,
        p50_gpu: 0,
        p95_gpu: 0,
    };
}

// ---------------------------------------------------------------------------
// 四、帧结算（五段计时 + 三水位 + 段级超支定位）
// ---------------------------------------------------------------------------

/// 段级超支定位结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegOverspend {
    /// 超支段。
    pub seg: PerfSegment,
    /// 实测（CPU 口径，µs）。
    pub used_us: u32,
    /// 该段分解档（µs）。
    pub budget_us: u32,
    /// 超支量（used - budget，>0）。
    pub over_us: u32,
}

/// 一帧的结算结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameSettle {
    /// 逻辑秒。
    pub tick: u64,
    /// 帧总账（CPU 口径，五段之和，µs）。
    pub total_cpu_us: u32,
    /// 帧总账（GPU 旁证，µs）。
    pub total_gpu_us: u32,
    /// 水位（复用 F0801 [`Water::judge`]，三水位判据）。
    pub water: Water,
    /// 最重超支段（多段同时超支取超支量最大者；无段超支为 None）。
    pub worst_overspend: Option<SegOverspend>,
    /// 双通道是否无分歧（false = 本帧至少一段分歧超限，已入账）。
    pub channel_ok: bool,
}

/// 一帧的五段双通道计时输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameTimers {
    /// 五段 CPU 读数（下标即 [`PerfSegment::ordinal`]）。
    pub cpu_us: [u32; 5],
    /// 五段 GPU 读数。
    pub gpu_us: [u32; 5],
}

impl FrameTimers {
    /// 从两数组构造（直传，不改口径）。
    pub const fn from_arrays(cpu_us: [u32; 5], gpu_us: [u32; 5]) -> FrameTimers {
        FrameTimers { cpu_us, gpu_us }
    }

    /// 取某段样本（下标越界不可能——枚举 ordinal 恒 0..5；防御式取 0）。
    pub fn sample(self, seg: PerfSegment) -> ChannelSample {
        let i = seg.ordinal();
        let cpu = match self.cpu_us.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let gpu = match self.gpu_us.get(i) {
            Some(v) => *v,
            None => 0,
        };
        ChannelSample { cpu_us: cpu, gpu_us: gpu }
    }
}

// ---------------------------------------------------------------------------
// 五、降级保护序（判据四；序与红线都是结构）
// ---------------------------------------------------------------------------

/// 可执行的降级步（F0801 `degradation_plan` 的执行化：名字与序同源引用，
/// 回收量是显性估计常量）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegradeExec {
    /// 序位（0 = 先做；与 F0801 计划的 order 一致）。
    pub order: usize,
    /// 步骤名（与 F0801 计划逐字一致——自检按名核对，防两处口径漂移）。
    pub name: String,
    /// 预计回收量（µs/帧；估计常量，不冒充实测）。
    pub save_us: u32,
    /// 是否触碰无障碍路径——三步恒 false（红线：想砍没有入口）。
    pub touches_a11y: bool,
}

/// 降级计划（红水位帧的处置结论）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegradePlan {
    /// 按序选中的步（未用尽时只列用到的；用尽时三条全列）。
    pub steps: Vec<DegradeExec>,
    /// 受保护的无障碍路径耗时（µs，原样保留——任何步都不得动它）。
    pub a11y_reserved_us: u32,
    /// 预计应用后的帧耗时（µs）。
    pub projected_us: u32,
    /// 目标水位线（回到黄：`WATER_YELLOW_MAX_US`）。
    pub target_us: u32,
    /// 是否达标（三步用尽仍 > 目标即 false，如实报出）。
    pub achieved: bool,
    /// 用户可见提示（降了多少、为什么、目标是什么——降级必须告知）。
    pub notice: String,
}

/// F0801 计划 → 可执行步（名字与序同源引用，回收量按序配常量）。
fn exec_steps(plan: &[DegradeStep]) -> Vec<DegradeExec> {
    let saves = [DEGRADE_SAVE_HINT_US, DEGRADE_SAVE_SUBPIXEL_US, DEGRADE_SAVE_DYNSIZE_US];
    let mut out: Vec<DegradeExec> = Vec::new();
    let mut i = 0usize;
    while i < plan.len() {
        if let Some(p) = plan.get(i) {
            let save = match saves.get(i) {
                Some(v) => *v,
                None => 0,
            };
            out.push(DegradeExec {
                order: p.order,
                name: p.name.clone(),
                save_us: save,
                touches_a11y: p.touches_a11y,
            });
        }
        i += 1;
    }
    out
}

/// 出降级计划：红水位帧 → 按 F0801 序选步，直到预计回到黄线。
///
/// `a11y_us` 是本帧无障碍路径（放大/高对比）的耗时：它**不参与任何步**，
/// 三步只作用于其余部分——"降级不得先砍无障碍"在这里是最强实现（结构上
/// 砍不到），红线名册 [`PROTECTED_A11Y_PATHS`] 供提示文案点名。
pub fn plan_degrade(used_us: u32, a11y_us: u32, target_us: u32) -> DegradePlan {
    let pipeline = TextPipeline::new();
    let all = exec_steps(&pipeline.degradation_plan());
    let target = if target_us == 0 { WATER_YELLOW_MAX_US } else { target_us };
    // 非无障碍部分才可被降级（红线是结构：a11y 恒保留）。
    let mut cur = used_us.saturating_sub(a11y_us);
    let mut chosen: Vec<DegradeExec> = Vec::new();
    let mut i = 0usize;
    while i < all.len() {
        if a11y_us.saturating_add(cur) <= target {
            break;
        }
        if let Some(step) = all.get(i) {
            cur = cur.saturating_sub(step.save_us);
            chosen.push(step.clone());
        }
        i += 1;
    }
    let projected = a11y_us.saturating_add(cur);
    let achieved = projected <= target;
    let notice = if achieved {
        format!(
            "【性能降级】文字渲染帧超水位（红），已按保护序启用 {} 步降级：预计 {}µs → {}µs（目标 ≤{}µs）。无障碍路径（{}/高对比）不受影响。",
            chosen.len(),
            used_us,
            projected,
            target,
            PROTECTED_A11Y_PATHS[0],
        )
    } else {
        format!(
            "【性能降级】文字渲染帧超水位（红），保护序 {} 步已全部启用仍预计 {}µs > 目标 {}µs——如实报未达标，无障碍路径（{}/高对比）仍然不受影响，需人工定位瓶颈段。",
            chosen.len(),
            projected,
            target,
            PROTECTED_A11Y_PATHS[0],
        )
    };
    DegradePlan {
        steps: chosen,
        a11y_reserved_us: a11y_us,
        projected_us: projected,
        target_us: target,
        achieved,
        notice,
    }
}

// ---------------------------------------------------------------------------
// 六、缓存诊断（命中率 <90% 持续 5 秒 → 自动出报告）
// ---------------------------------------------------------------------------

/// 缓存诊断报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheDiag {
    /// 开始跌破 90% 的逻辑秒。
    pub since_tick: u64,
    /// 报告产出的逻辑秒（持续满 [`DIAG_SUSTAIN_SECS`] 的那一秒）。
    pub at_tick: u64,
    /// 跌破期间的最差命中率（百万分比）。
    pub worst_ppm: u32,
}

/// 缓存命中率监视器（逻辑秒注入；恢复清账，可再触发）。
#[derive(Clone, Debug, PartialEq, Eq)]
struct CacheWatch {
    below_since: Option<u64>,
    worst_while_below: u32,
    active: Option<CacheDiag>,
    reports: u32,
    last_ppm: u32,
}

impl CacheWatch {
    const fn new() -> CacheWatch {
        CacheWatch {
            below_since: None,
            worst_while_below: u32::MAX,
            active: None,
            reports: 0,
            last_ppm: 0,
        }
    }

    /// 喂一次命中率采样（queries==0 视为无数据：不触发也不清账）。
    fn feed(&mut self, tick: u64, queries: u64, hits: u64) -> Option<CacheDiag> {
        if queries == 0 {
            return None;
        }
        let ppm = ((hits.min(u64::MAX / 2) * 1_000_000) / queries) as u32;
        self.last_ppm = ppm;
        if ppm >= DIAG_HIT_RATE_PPM {
            // 恢复（含恰好等于 90% 的边界：≥ 不算跌破）。
            self.below_since = None;
            self.worst_while_below = u32::MAX;
            self.active = None;
            return None;
        }
        let since = match self.below_since {
            Some(s) => s,
            None => {
                self.below_since = Some(tick);
                self.worst_while_below = ppm;
                tick
            }
        };
        if ppm < self.worst_while_below {
            self.worst_while_below = ppm;
        }
        if tick.saturating_sub(since) >= DIAG_SUSTAIN_SECS && self.active.is_none() {
            let diag = CacheDiag {
                since_tick: since,
                at_tick: tick,
                worst_ppm: self.worst_while_below,
            };
            self.active = Some(diag);
            self.reports += 1;
            return Some(diag);
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 七、记录器（样本环 + 结算 + 汇总）
// ---------------------------------------------------------------------------

/// 文字渲染性能记录器：接收运行时注入的段级双通道样本，负责口径、判定
/// 与汇总。逻辑秒（`tick`）由调用方注入，零墙钟、可回放复现。
#[derive(Clone, Debug)]
pub struct PerfRecorder {
    tick: u64,
    seg_cpu: [Vec<u32>; 5],
    seg_gpu: [Vec<u32>; 5],
    dropped_samples: u32,
    mismatches: Vec<ChannelMismatch>,
    frames: Vec<FrameSettle>,
    red_frames: u32,
    cache: CacheWatch,
}

impl PerfRecorder {
    /// 新建（逻辑秒 0 起）。
    pub fn new() -> PerfRecorder {
        PerfRecorder {
            tick: 0,
            seg_cpu: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            seg_gpu: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            dropped_samples: 0,
            mismatches: Vec::new(),
            frames: Vec::new(),
            red_frames: 0,
            cache: CacheWatch::new(),
        }
    }

    /// 当前逻辑秒。
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// 段样本环容量纪律：新样本必进，环满时挤掉**最旧**样本
    /// （被挤掉的计数入账，零静默）——滑动窗保最新： recent 测量才反映当下。
    fn push_bounded(ring: &mut Vec<u32>, v: u32) -> bool {
        if ring.len() >= MAX_SAMPLES_PER_SEG {
            if !ring.is_empty() {
                ring.remove(0);
            }
            ring.push(v);
            true
        } else {
            ring.push(v);
            false
        }
    }

    /// 记一帧（五段双通道）→ 结算结论。
    ///
    /// 结算口径恒为 CPU；GPU 分歧超限记 [`ChannelMismatch`] 不静默。
    pub fn record_frame(&mut self, tick: u64, timers: FrameTimers) -> FrameSettle {
        self.tick = tick;
        let mut total_cpu = 0u32;
        let mut total_gpu = 0u32;
        let mut channel_ok = true;
        let mut si = 0usize;
        while si < PerfSegment::ALL.len() {
            let seg = match PerfSegment::ALL.get(si) {
                Some(s) => *s,
                None => break,
            };
            let smp = timers.sample(seg);
            total_cpu = total_cpu.saturating_add(smp.cpu_us);
            total_gpu = total_gpu.saturating_add(smp.gpu_us);
            if smp.cpu_us.abs_diff(smp.gpu_us) > CHANNEL_DRIFT_MAX_US {
                self.mismatches.push(ChannelMismatch {
                    tick,
                    seg,
                    cpu_us: smp.cpu_us,
                    gpu_us: smp.gpu_us,
                });
                channel_ok = false;
            }
            if Self::push_bounded(&mut self.seg_cpu[si], smp.cpu_us) {
                self.dropped_samples += 1;
            }
            if Self::push_bounded(&mut self.seg_gpu[si], smp.gpu_us) {
                self.dropped_samples += 1;
            }
            si += 1;
        }
        let water = Water::judge(total_cpu);
        // 段级超支定位：总账不超也可能段级越档——总账是掩盖段级劣化的
        // 最常见藏身处，定位到段才算看清。
        let mut worst: Option<SegOverspend> = None;
        let mut wi = 0usize;
        while wi < PerfSegment::ALL.len() {
            let seg = match PerfSegment::ALL.get(wi) {
                Some(s) => *s,
                None => break,
            };
            let used = match timers.cpu_us.get(wi) {
                Some(v) => *v,
                None => 0,
            };
            let budget = seg.budget_us();
            if used > budget {
                let over = used - budget;
                let replace = match worst {
                    None => true,
                    Some(w) => over > w.over_us,
                };
                if replace {
                    worst = Some(SegOverspend {
                        seg,
                        used_us: used,
                        budget_us: budget,
                        over_us: over,
                    });
                }
            }
            wi += 1;
        }
        if water == Water::Red {
            self.red_frames += 1;
        }
        let settle = FrameSettle {
            tick,
            total_cpu_us: total_cpu,
            total_gpu_us: total_gpu,
            water,
            worst_overspend: worst,
            channel_ok,
        };
        // 结算留档滑动窗（超限挤最旧，与样本环同纪律）。
        if self.frames.len() >= MAX_SETTLED_FRAMES && !self.frames.is_empty() {
            self.frames.remove(0);
        }
        self.frames.push(settle.clone());
        settle
    }

    /// 喂缓存命中率采样（queries==0 无数据；跌破 90% 持续 5 逻辑秒自动出报告）。
    pub fn cache_sample(&mut self, tick: u64, queries: u64, hits: u64) -> Option<CacheDiag> {
        self.cache.feed(tick, queries, hits)
    }

    /// 某段统计（P50/P95 双通道分列；无样本给 0）。
    pub fn seg_stats(&self, seg: PerfSegment) -> SegStats {
        let i = seg.ordinal();
        let cpu = match self.seg_cpu.get(i) {
            Some(v) => v,
            None => return SegStats::EMPTY,
        };
        let gpu = match self.seg_gpu.get(i) {
            Some(v) => v,
            None => return SegStats::EMPTY,
        };
        SegStats {
            n: cpu.len(),
            p50_cpu: percentile_of(cpu, 50),
            p95_cpu: percentile_of(cpu, 95),
            p50_gpu: percentile_of(gpu, 50),
            p95_gpu: percentile_of(gpu, 95),
        }
    }

    /// 通道分歧账（全量）。
    pub fn mismatches(&self) -> &[ChannelMismatch] {
        self.mismatches.as_slice()
    }

    /// 结算帧留档。
    pub fn frames(&self) -> &[FrameSettle] {
        self.frames.as_slice()
    }

    /// 被挤掉的样本数（容量纪律的观测面）。
    pub fn dropped_samples(&self) -> u32 {
        self.dropped_samples
    }

    /// 红水位帧累计（降级压力的观测面）。
    pub fn red_frames(&self) -> u32 {
        self.red_frames
    }

    /// 缓存诊断已出报告次数。
    pub fn cache_reports(&self) -> u32 {
        self.cache.reports
    }

    /// 最近一次命中率（百万分比；无数据给 0）。
    pub fn last_cache_ppm(&self) -> u32 {
        self.cache.last_ppm
    }

    /// 基准场景名（常量同源引用 F0812，不重抄数值）。
    pub fn bench_scenario() -> String {
        format!(
            "F0812 基准场景（{} 静态字 + {} 动态字）",
            BENCH_STATIC_GLYPHS, BENCH_DYNAMIC_GLYPHS
        )
    }

    /// Eb07 冻结汇总表（Eb07 只汇总不重测——见 [`EB07_DOC`]）。
    pub fn summary_for_eb07(&self) -> PerfSummary {
        let mut table: Vec<(String, u32)> = Vec::new();
        let mut i = 0usize;
        while i < BUDGET_SLICES.len() {
            if let Some((name, us)) = BUDGET_SLICES.get(i) {
                table.push(((*name).to_string(), *us));
            }
            i += 1;
        }
        let mut stats: [SegStats; 5] =
            [SegStats::EMPTY, SegStats::EMPTY, SegStats::EMPTY, SegStats::EMPTY, SegStats::EMPTY];
        let mut si = 0usize;
        while si < PerfSegment::ALL.len() {
            if let Some(seg) = PerfSegment::ALL.get(si) {
                stats[si] = self.seg_stats(*seg);
            }
            si += 1;
        }
        PerfSummary {
            scenario: Self::bench_scenario(),
            budget_table: table,
            seg_stats: stats,
            frames_settled: self.frames.len(),
            red_frames: self.red_frames,
            channel_mismatches: self.mismatches.len(),
            cache_reports: self.cache.reports,
        }
    }
}

impl Default for PerfRecorder {
    fn default() -> PerfRecorder {
        PerfRecorder::new()
    }
}

/// Eb07 汇总表（冻结口径）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PerfSummary {
    /// 测量场景（F0812 基准场景，常量同源）。
    pub scenario: String,
    /// 预算表（与 F0801 `BUDGET_SLICES` 逐格一致）。
    pub budget_table: Vec<(String, u32)>,
    /// 五段 P50/P95 分列统计。
    pub seg_stats: [SegStats; 5],
    /// 已结算帧数（滑动窗内）。
    pub frames_settled: usize,
    /// 红水位帧累计。
    pub red_frames: u32,
    /// 通道分歧累计。
    pub channel_mismatches: usize,
    /// 缓存诊断报告累计。
    pub cache_reports: u32,
}

// ---------------------------------------------------------------------------
// 八、域自检（判据区零 panic 面；反向语料钉门禁不恒绿）
// ---------------------------------------------------------------------------

/// 判据侧独立重排的锚点判据五条。
const CRITERIA_RECHECK: [&str; 5] = [
    "1.5ms 分解",
    "五段计时",
    "三水位",
    "降级保护序",
    "双通道计时",
];

/// 独立重排的六段分解档字面值（与 F0801 `BUDGET_SLICES` 对账）。
const SLICES_RECHECK: [(&str, u32); 6] = [
    ("解码", 50),
    ("光栅化", 500),
    ("整形", 300),
    ("图元渲染", 400),
    ("遥测", 20),
    ("余量", 230),
];

/// VE-F0815 域自检入口（聚合器 `run_svstar2_checks` 调用）。
pub fn run_vee15_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("vee15_perf");

    // —— 判据一 · 1.5ms 分解：同源对账（独立重算，常量被误改先红）——
    let pipeline = TextPipeline::new();
    let mut sum = 0u32;
    let mut slices_ok = BUDGET_SLICES.len() == SLICES_RECHECK.len();
    let mut i = 0usize;
    while i < SLICES_RECHECK.len() {
        let (name, us) = SLICES_RECHECK[i];
        sum = sum.saturating_add(us);
        match BUDGET_SLICES.get(i) {
            Some((n2, v2)) => {
                if *n2 != name || *v2 != us {
                    slices_ok = false;
                }
            }
            None => slices_ok = false,
        }
        i += 1;
    }
    s.add(
        "A46-预算分解-六段同源对账",
        slices_ok
            && sum == FRAME_BUDGET_US
            && FRAME_BUDGET_US == 1500
            && pipeline.budget_consistent(),
        "六段分解与 F0801 逐格全等、和恰 1500µs、F0801 自洽核验同过",
    );

    // —— 判据二 · 五段计时：段映射按名+按值双对位（防错位与漂移）——
    let mut map_ok = PerfSegment::ALL.len() == 5;
    let mut mi = 0usize;
    while mi < 5 {
        let seg = match PerfSegment::ALL.get(mi) {
            Some(v) => *v,
            None => {
                map_ok = false;
                break;
            }
        };
        let (name, us) = match SLICES_RECHECK.get(mi) {
            Some(v) => *v,
            None => {
                map_ok = false;
                break;
            }
        };
        if seg.label() != name || seg.budget_us() != us || seg.ordinal() != mi {
            map_ok = false;
        }
        mi += 1;
    }
    s.add(
        "A46-五段计时-段档位映射逐段对位",
        map_ok,
        "五段名与值逐段对位 BUDGET_SLICES[0..5]；「余量」不计入计时面",
    );

    // —— 场景固定：F0812 基准场景常量同源引用 ——
    let label = PerfRecorder::bench_scenario();
    s.add(
        "A46-场景固定-基准场景同源",
        BENCH_STATIC_GLYPHS == 1_000
            && BENCH_DYNAMIC_GLYPHS == 500
            && label.contains("1000")
            && label.contains("500")
            && label.contains("F0812"),
        "场景常量引用 F0812（1000 静态+500 动态），呈现含场景名与字数",
    );

    // —— 判据三 · 三水位：复用 F0801 judge，边界夹逼 ——
    let water_ok = Water::judge(WATER_GREEN_MAX_US) == Water::Green
        && Water::judge(WATER_GREEN_MAX_US + 1) == Water::Yellow
        && Water::judge(WATER_YELLOW_MAX_US) == Water::Yellow
        && Water::judge(WATER_YELLOW_MAX_US + 1) == Water::Red
        && WATER_GREEN_MAX_US == 300
        && WATER_YELLOW_MAX_US == 750;
    s.add(
        "A46-三水位-阈值边界夹逼",
        water_ok,
        "300/750 阈值两侧逐点判位（恰等归下档），阈值与 F0801 常量同源",
    );

    // —— 判据五 · 双通道：正常分歧不报，超限入账且结算仍走 CPU ——
    let mut rec = PerfRecorder::new();
    let nominal = FrameTimers::from_arrays([12, 180, 60, 40, 6], [13, 185, 58, 41, 6]);
    let st1 = rec.record_frame(1, nominal);
    let drift = FrameTimers::from_arrays([12, 200, 60, 40, 6], [12, 405, 60, 40, 6]);
    let st2 = rec.record_frame(2, drift);
    s.add(
        "A46-双通道-分歧阈值与结算口径",
        st1.channel_ok
            && !st2.channel_ok
            && st2.total_cpu_us == 318
            && rec.mismatches().len() == 1
            && CHANNEL_DRIFT_MAX_US == 150,
        "正常噪声不报；GPU 分歧 205µs > 150µs 入账一段、帧结算仍以 CPU 为准",
    );

    // —— P50/P95 分列：升序语料使 P50≠P95（恒等语料会放走秩位漂移）——
    let mut rec2 = PerfRecorder::new();
    let mut k = 0u32;
    while k < 10 {
        let v = k + 1; // 1..=10 升序：P50=5、P95=10，两秩位不同
        rec2.record_frame(
            k as u64,
            FrameTimers::from_arrays([v, 0, 0, 0, 0], [v * 2, 0, 0, 0, 0]),
        );
        k += 1;
    }
    let st = rec2.seg_stats(PerfSegment::Decode);
    s.add(
        "A46-分位-P50P95分列且取秩正确",
        st.n == 10
            && st.p50_cpu == 5
            && st.p95_cpu == 10
            && st.p50_gpu == 10
            && st.p95_gpu == 20
            && st.p95_cpu > st.p50_cpu
            && rec2.seg_stats(PerfSegment::Raster).n == 0
            && rec2.seg_stats(PerfSegment::Raster).p50_cpu == 0,
        "升序语料 P50=5/P95=10（GPU 通道独立 10/20）；无样本段给 0（F0809 同口径）",
    );

    // —— 段级超支定位：总账未超但光栅越档，必须点名光栅 ——
    let mut rec3 = PerfRecorder::new();
    let hidden = FrameTimers::from_arrays([40, 620, 280, 380, 18], [40, 620, 280, 380, 18]);
    let st3 = rec3.record_frame(3, hidden);
    let located = match st3.worst_overspend {
        Some(w) => {
            w.seg == PerfSegment::Raster && w.used_us == 620 && w.budget_us == 500 && w.over_us == 120
        }
        None => false,
    };
    s.add(
        "A46-超支定位-段级越档点名",
        st3.total_cpu_us == 1338
            && st3.total_cpu_us <= FRAME_BUDGET_US
            && located,
        "总账 1338µs 未超 1500 但光栅 620>500：定位结论=光栅段超 120µs（总账不掩盖段级劣化）",
    );

    // —— 判据四 · 降级保护序：序与名同源 F0801，红线三步恒 false ——
    let plan_src = pipeline.degradation_plan();
    let exec = exec_steps(&plan_src);
    let mut order_ok = plan_src.len() == 3 && exec.len() == 3;
    let mut pi = 0usize;
    while pi < 3 {
        match (plan_src.get(pi), exec.get(pi)) {
            (Some(p), Some(e)) => {
                if p.order != pi as usize || e.name != p.name || e.touches_a11y || p.touches_a11y {
                    order_ok = false;
                }
            }
            _ => order_ok = false,
        }
        pi += 1;
    }
    s.add(
        "A46-降级序-三步同源且红线恒假",
        order_ok
            && exec.get(0).map(|e| e.save_us) == Some(DEGRADE_SAVE_HINT_US)
            && exec.get(1).map(|e| e.save_us) == Some(DEGRADE_SAVE_SUBPIXEL_US)
            && exec.get(2).map(|e| e.save_us) == Some(DEGRADE_SAVE_DYNSIZE_US),
        "三步名字与序逐字同源 F0801 计划；touches_a11y 三步恒 false（想砍没有入口）",
    );

    // —— 降级计划：红帧两步回到黄线；恰等目标即达标 ——
    let plan = plan_degrade(900, 0, 0);
    s.add(
        "A46-降级计划-红帧按序回收达标",
        plan.steps.len() == 2
            && plan.projected_us == 700
            && plan.target_us == WATER_YELLOW_MAX_US
            && plan.achieved
            && plan.a11y_reserved_us == 0
            && plan.notice.contains("2 步")
            && plan.notice.contains("900")
            && plan.notice.contains("700"),
        "900µs 红帧：①120+②80 回收至 700≤750 达标；提示含步数与前后值（降级必须告知）",
    );

    // —— 无障碍红线：a11y 耗时结构性保留，降级只作用于其余部分 ——
    let plan_a11y = plan_degrade(900, 200, 0);
    s.add(
        "A46-红线-无障碍耗时结构性保留",
        plan_a11y.a11y_reserved_us == 200
            && plan_a11y.projected_us == 700
            && plan_a11y.achieved
            && plan_a11y.notice.contains("不受影响"),
        "900=200(a11y)+700：两步作用于 700 部分，projected=200+500=700，a11y 一微秒未动",
    );

    // —— 反向语料：三步用尽仍不达标必须如实报（不假装救回）——
    let plan_hard = plan_degrade(2000, 0, 0);
    s.add(
        "A46-降级计划-用尽如实报未达标",
        plan_hard.steps.len() == 3
            && plan_hard.projected_us == 1740
            && !plan_hard.achieved
            && plan_hard.notice.contains("未达标"),
        "2000µs 极端帧三步全用仍 1740>750：achieved=false 且提示点名未达标与需人工定位",
    );

    // —— 缓存诊断：跌破 90% 持续 5 秒自动出报告；恢复清账可再触发 ——
    let mut rec4 = PerfRecorder::new();
    let quiet = rec4.cache_sample(1, 1000, 960);
    let mut r1: Option<CacheDiag> = None;
    let mut t = 10u64;
    while t <= 14 {
        r1 = rec4.cache_sample(t, 1000, 850);
        t += 1;
    }
    let boundary = rec4.cache_sample(20, 1000, 900);
    let recover = rec4.cache_sample(21, 1000, 950);
    let again = rec4.cache_sample(30, 1000, 800);
    let second = rec4.cache_sample(35, 1000, 790);
    let no_data = rec4.cache_sample(36, 0, 0);
    s.add(
        "A46-缓存诊断-持续触发与恢复再报",
        quiet.is_none()
            && r1.is_none()
            && rec4.cache_sample(15, 1000, 850).is_some()
            && boundary.is_none()
            && recover.is_none()
            && again.is_none()
            && second.is_some()
            && no_data.is_none()
            && rec4.cache_reports() == 2
            && DIAG_HIT_RATE_PPM == 900_000
            && DIAG_SUSTAIN_SECS == 5,
        "85% 第 5 秒出报告（第 4 秒不出）；恰 90% 不触发；恢复后再跌可再报；查表 0 次无数据不动状态",
    );

    // —— 容量纪律：样本环挤旧入账，结算窗同纪律 ——
    let mut rec5 = PerfRecorder::new();
    let mut n5 = 0u32;
    while n5 < (MAX_SAMPLES_PER_SEG as u32) + 8 {
        rec5.record_frame(
            n5 as u64,
            FrameTimers::from_arrays([10, 10, 10, 10, 10], [10, 10, 10, 10, 10]),
        );
        n5 += 1;
    }
    s.add(
        "A46-容量-样本环挤旧入账",
        rec5.dropped_samples() == 16
            && rec5.seg_stats(PerfSegment::Decode).n == MAX_SAMPLES_PER_SEG
            && rec5.frames().len() == MAX_SETTLED_FRAMES,
        "每段 264 进 256 留：挤掉 8×2 通道=16 全部计数；结算窗恒 ≤64 不膨胀",
    );

    // —— Eb07 单源：冻结表与预算同源、统计非平凡 ——
    let summ = rec5.summary_for_eb07();
    let table_ok = summ.budget_table.len() == 6
        && summ.budget_table.get(1).map(|p| *p == ("光栅化".to_string(), 500)).unwrap_or(false);
    s.add(
        "A46-Eb07-冻结表同源非平凡",
        table_ok
            && summ.frames_settled == MAX_SETTLED_FRAMES
            && summ.scenario.contains("F0812")
            && EB07_DOC.contains("只汇总不重测")
            && REUSE_DOC.contains("P07"),
        "汇总表六格与 F0801 同源、场景名同源；对接声明写明 Eb07 只汇总不重测",
    );

    // —— 判据 stamp 独立对账 ——
    let mut stamp_ok = true;
    let stamps = ["1.5ms 分解", "五段计时", "三水位", "降级保护序", "双通道计时"];
    let mut ci = 0usize;
    while ci < stamps.len() {
        if CRITERIA_RECHECK.get(ci) != Some(&stamps[ci]) {
            stamp_ok = false;
        }
        ci += 1;
    }
    s.add(
        "A46-判据stamp-五条独立重排全等",
        stamp_ok && CRITERIA_RECHECK.len() == 5,
        "锚点判据五条与判据侧独立重排逐条全等（常量被误改先红）",
    );

    s
}
