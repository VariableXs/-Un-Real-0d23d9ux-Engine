//! CGPU-F0481 · D 域开工与 80 帧合同模型（CGPU-D 域 · 80 帧保真 · 开山单）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0481`
//!
//! 锚点原文：「D 域开工：80 帧保真系统域（F0481-F0640，官方八主题：80fps
//! 合同/帧计时/抖动抑制/卡顿归因/掉帧降质链/帧间隔均衡/输入优先/承诺报告）；
//! 合同正式化（基准场景 30 网页+多路 4K 动图：P95≥80fps/P99≥72fps/零
//! 250ms+ 卡顿——每条可测可判）；C 域移交包对接（F0476 预算侧契约落地
//! ——帧计时反馈闭环启用）；降质不降帧铁律重申（做不到先降质、降帧即
//! 事故——域级最高纪律）。判据：八主题、可测条款、对接启用、铁律重申、
//! 判据。」
//!
//! # 一、合同正式化 = 每条承诺**可测可判**
//!
//! 「流畅」不可测，所以不是合同条款。三条条款全部落成判定函数：
//! P95 帧率 ≥80（[`CLAUSE_P95_FPS`]）、P99 帧率 ≥72（[`CLAUSE_P99_FPS`]）、
//! 250ms+ 卡顿零发生（[`CLAUSE_STALL_MS`]）——输入帧间隔样本序列，
//! 输出逐条通过/违约与违约证据，[`ContractVerdict`] 一锤定音。
//!
//! # 二、降质不降帧：铁律落成**阶梯与事故**
//!
//! 帧率撑不住时先降质（[`DEGRADE_ORDER`] 五级阶梯：特效→分辨率→精度，
//! 全降完才许谈帧），任何「以降帧换稳定」的处置都是**事故**
//! （[`E_D481_FRAME_DROP`]）——域级最高纪律不做口号，做成类型与计数。
//!
//! # 三、C 域移交对接：帧计时反馈闭环
//!
//! F0476 预算侧契约落地形态：[`FrameTimingFeedback`] 启用后每帧耗时
//! O(1) 入环（[`FrameTimingFeedback::record`]），预算侧可读——闭环
//! 「启用」是状态可查的，不是文档里的一句话。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误契约：独占 0x48 细分段
// ---------------------------------------------------------------------------

/// 合同违约（判定运行了且有条目不过——证据在 [`ContractVerdict`]）。
pub const E_D481_CLAUSE_FAIL: u16 = 0x4800;
/// 帧计时反馈未启用即请求判定（闭环没开就别谈合同）。
pub const E_D481_TIMING_OFF: u16 = 0x4801;
/// 降帧事故（铁律：降帧即事故）。
pub const E_D481_FRAME_DROP: u16 = 0x4802;
/// 样本不足（分位判定最少样本数——样本不足的 P95 是噪声）。
pub const E_D481_SAMPLE_SHORT: u16 = 0x4803;

// ---------------------------------------------------------------------------
// 八主题封闭枚举
// ---------------------------------------------------------------------------

/// 80 帧保真系统域官方八主题（封闭全集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FidelityTheme {
    /// 80fps 合同。
    Contract,
    /// 帧计时。
    FrameTiming,
    /// 抖动抑制。
    JitterDampen,
    /// 卡顿归因。
    StallAttribution,
    /// 掉帧降质链。
    DropDegradChain,
    /// 帧间隔均衡。
    IntervalBalance,
    /// 输入优先。
    InputPriority,
    /// 承诺报告。
    PromiseReport,
}

impl FidelityTheme {
    /// 全枚举（顺序即下标）。
    pub const ALL: [FidelityTheme; 8] = [
        FidelityTheme::Contract,
        FidelityTheme::FrameTiming,
        FidelityTheme::JitterDampen,
        FidelityTheme::StallAttribution,
        FidelityTheme::DropDegradChain,
        FidelityTheme::IntervalBalance,
        FidelityTheme::InputPriority,
        FidelityTheme::PromiseReport,
    ];

    /// 枚举下标。
    pub const fn ordinal(self) -> usize {
        match self {
            FidelityTheme::Contract => 0,
            FidelityTheme::FrameTiming => 1,
            FidelityTheme::JitterDampen => 2,
            FidelityTheme::StallAttribution => 3,
            FidelityTheme::DropDegradChain => 4,
            FidelityTheme::IntervalBalance => 5,
            FidelityTheme::InputPriority => 6,
            FidelityTheme::PromiseReport => 7,
        }
    }

    /// 下标 → 枚举（越界 None）。
    pub const fn of_ordinal(i: usize) -> Option<FidelityTheme> {
        match i {
            0 => Some(FidelityTheme::Contract),
            1 => Some(FidelityTheme::FrameTiming),
            2 => Some(FidelityTheme::JitterDampen),
            3 => Some(FidelityTheme::StallAttribution),
            4 => Some(FidelityTheme::DropDegradChain),
            5 => Some(FidelityTheme::IntervalBalance),
            6 => Some(FidelityTheme::InputPriority),
            7 => Some(FidelityTheme::PromiseReport),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 合同条款（可测可判）
// ---------------------------------------------------------------------------

/// 条款一：P95 帧率下限（fps）。
pub const CLAUSE_P95_FPS: u32 = 80;
/// 条款二：P99 帧率下限（fps）。
pub const CLAUSE_P99_FPS: u32 = 72;
/// 条款三：卡顿阈值（毫秒，250ms+ 计卡顿，零容忍）。
pub const CLAUSE_STALL_MS: u32 = 250;
/// 分位判定最少样本数（不足则拒绝判定——样本不足的 P95 是噪声）。
pub const MIN_SAMPLES: usize = 100;

/// 单条款判定结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClauseResult {
    /// 条款名（读屏可达）。
    pub name: &'static str,
    /// 是否通过。
    pub passed: bool,
    /// 实测值（P95/P99 为 fps；卡顿条款为卡顿发生次数）。
    pub measured: u32,
}

/// 合同判定总表。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractVerdict {
    /// 三条款逐条结果。
    pub clauses: [ClauseResult; 3],
    /// 整体是否兑现（三条全过）。
    pub fulfilled: bool,
}

/// 升序插入排序（判据语料非恒等——升序语料使 P95≠P99 可测）。
fn sort_asc(samples: &mut Vec<u32>) {
    let n = samples.len();
    let mut i = 1usize;
    while i < n {
        let mut j = i;
        while j > 0 && samples[j - 1] > samples[j] {
            samples.swap(j - 1, j);
            j -= 1;
        }
        i += 1;
    }
}

/// 秩取分位（升序后取 `ceil(p*n)-1` 秩位；最近帧率 = 1e6/间隔 us）。
fn percentile_fps(sorted: &Vec<u32>, p_permille: u32) -> u32 {
    let n = sorted.len();
    if n == 0 {
        return 0;
    }
    let rank = ((p_permille as u64 * n as u64) + 999) / 1000;
    let rank = if rank == 0 { 1usize } else { rank as usize };
    let idx = (rank - 1).min(n - 1);
    let interval_us = sorted[idx] as u64;
    if interval_us == 0 {
        return 0;
    }
    (1_000_000 / interval_us) as u32
}

/// **evaluate**（合同判定）：帧间隔样本（微秒，任意序）→ 三条款判定。
///
/// 排序一次三条款共享；样本不足拒绝判定（不产出噪声结论）。
pub fn evaluate(samples: &[u32]) -> Result<ContractVerdict, u16> {
    if samples.len() < MIN_SAMPLES {
        return Err(E_D481_SAMPLE_SHORT);
    }
    let mut sorted = Vec::with_capacity(samples.len());
    for s in samples.iter() {
        sorted.push(*s);
    }
    sort_asc(&mut sorted);
    let p95 = percentile_fps(&sorted, 950);
    let p99 = percentile_fps(&sorted, 990);
    // 卡顿扫描：250ms+（250_000us+）发生次数。
    let mut stalls = 0u32;
    for s in sorted.iter() {
        if *s >= CLAUSE_STALL_MS * 1000 {
            stalls += 1;
        }
    }
    let c1 = ClauseResult { name: "P95>=80fps", passed: p95 >= CLAUSE_P95_FPS, measured: p95 };
    let c2 = ClauseResult { name: "P99>=72fps", passed: p99 >= CLAUSE_P99_FPS, measured: p99 };
    let c3 = ClauseResult {
        name: "零250ms+卡顿",
        passed: stalls == 0,
        measured: stalls,
    };
    let fulfilled = c1.passed && c2.passed && c3.passed;
    Ok(ContractVerdict { clauses: [c1, c2, c3], fulfilled })
}

// ---------------------------------------------------------------------------
// 帧计时反馈闭环（C 域 F0476 契约落地）
// ---------------------------------------------------------------------------

/// 反馈环容量（环形，满则覆盖最旧——与全域环形账同范式）。
pub const TIMING_RING_CAP: usize = 256;
/// C 域移交契约名（F0476 预算侧契约落地对接面）。
pub const C_HANDOFF_CONTRACT: &str = "F0476:frame-timing-feedback";

/// 帧计时反馈闭环（启用是可查状态；O(1) 入环）。
#[derive(Clone, Debug)]
pub struct FrameTimingFeedback {
    enabled: bool,
    ring: Vec<u32>,
    head: usize,
    total: u64,
    frames: u64,
}

impl FrameTimingFeedback {
    /// 新闭环（默认未启用——启用必须是显式动作）。
    pub fn new() -> FrameTimingFeedback {
        FrameTimingFeedback {
            enabled: false,
            ring: Vec::new(),
            head: 0,
            total: 0,
            frames: 0,
        }
    }

    /// 启用闭环（C 域契约落地动作）。
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// 闭环是否启用（状态可查——不是文档里的一句话）。
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// **record**（O(1)）：一帧耗时入环（us）。未启用时拒绝。
    pub fn record(&mut self, frame_us: u32) -> Result<(), u16> {
        if !self.enabled {
            return Err(E_D481_TIMING_OFF);
        }
        if self.ring.len() < TIMING_RING_CAP {
            self.ring.push(frame_us);
        } else {
            self.ring[self.head] = frame_us;
            self.head = (self.head + 1) % TIMING_RING_CAP;
        }
        self.total += frame_us as u64;
        self.frames += 1;
        Ok(())
    }

    /// 环内样本快照（供判定；环满时按逻辑序）。
    pub fn snapshot(&self) -> Vec<u32> {
        let mut out = Vec::with_capacity(self.ring.len());
        if self.ring.len() < TIMING_RING_CAP {
            for v in self.ring.iter() {
                out.push(*v);
            }
        } else {
            let mut i = self.head;
            let mut n = 0usize;
            while n < TIMING_RING_CAP {
                out.push(self.ring[i]);
                i = (i + 1) % TIMING_RING_CAP;
                n += 1;
            }
        }
        out
    }

    /// 平均帧耗时（us；无样本 0）。
    pub const fn avg_frame_us(&self) -> u64 {
        if self.frames == 0 {
            0
        } else {
            self.total / self.frames
        }
    }
}

// ---------------------------------------------------------------------------
// 降质不降帧铁律（域级最高纪律）
// ---------------------------------------------------------------------------

/// 降质阶梯（先降的先列——全降完才许谈帧）。
pub const DEGRADE_ORDER: [&str; 5] =
    ["特效档", "后处理", "分辨率", "纹理精度", "阴影档"];

/// 降质控制器：阶梯逐级降，帧率恢复即停；降帧 = 事故。
#[derive(Clone, Debug)]
pub struct DegradeGuard {
    /// 当前降质级（0 = 全开；最大 DEGRADE_ORDER.len()）。
    pub level: usize,
    /// 降帧事故计数（铁律：降帧即事故——每次降帧处置都记账）。
    pub frame_drop_incidents: u32,
    /// 降质步数（降质动作计数——先降质的证据）。
    pub degrade_steps: u32,
}

impl DegradeGuard {
    /// 全开起步。
    pub fn new() -> DegradeGuard {
        DegradeGuard { level: 0, frame_drop_incidents: 0, degrade_steps: 0 }
    }

    /// **degrade_step**（帧率撑不住时调用）：降一级质（O(1)）。
    ///
    /// 全降完（level 已到顶）再被要求降级——那已经不是降质能解决的，
    /// 返回 `Err(E_D481_FRAME_DROP)` 并记事故（铁律落点：降帧即事故）。
    pub fn degrade_step(&mut self) -> Result<usize, u16> {
        if self.level >= DEGRADE_ORDER.len() {
            // 阶梯到底还撑不住 = 只能降帧 = 事故。
            self.frame_drop_incidents += 1;
            return Err(E_D481_FRAME_DROP);
        }
        self.level += 1;
        self.degrade_steps += 1;
        Ok(self.level)
    }

    /// 当前降质档名（读屏可达）。
    pub fn current_label(&self) -> String {
        if self.level == 0 {
            String::from("全开")
        } else {
            String::from(DEGRADE_ORDER[self.level - 1])
        }
    }

    /// 铁律是否完好（零事故）。
    pub const fn ironlaw_intact(&self) -> bool {
        self.frame_drop_incidents == 0
    }
}

// ---------------------------------------------------------------------------
// 编译期闸
// ---------------------------------------------------------------------------

const _: () = {
    assert!(CLAUSE_P95_FPS == 80);
    assert!(CLAUSE_P99_FPS == 72);
    assert!(CLAUSE_STALL_MS == 250);
    assert!(MIN_SAMPLES == 100);
    assert!(CLAUSE_P99_FPS < CLAUSE_P95_FPS);
    assert!(TIMING_RING_CAP == 256);
    assert!(DEGRADE_ORDER.len() == 5);
    assert!(E_D481_CLAUSE_FAIL & 0xFF00 == 0x4800);
    assert!(E_D481_TIMING_OFF & 0xFF00 == 0x4800);
    assert!(E_D481_FRAME_DROP & 0xFF00 == 0x4800);
    assert!(E_D481_SAMPLE_SHORT & 0xFF00 == 0x4800);
    assert!(
        E_D481_CLAUSE_FAIL != E_D481_TIMING_OFF
            && E_D481_TIMING_OFF != E_D481_FRAME_DROP
            && E_D481_FRAME_DROP != E_D481_SAMPLE_SHORT
    );
};
