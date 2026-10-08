//! VE-F0225 · Intel 显示控制器（pipe/plane）（VE-B 域 · Intel 核显组 · 目标 460 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0225`
//!
//! **判据（锚点原文五条）**：pipe 层级、时序匹配、watermark、原子序、判据。
//!
//! 显示控制器按 **pipe→plane 两级**组织：每个 pipe 挂**主 plane + SPR
//! （精灵）+ CUR（光标）**三个 plane 槽（层级固定，缺槽位是拓扑事实不是
//! 配置项）。时序编程校验 **link rate × lanes 与模式时钟的带宽匹配**——
//! 带宽不够的时序会在点亮后表现为花屏或黑屏而非报错，故必须在编程口
//! 拒绝。watermark 按 **DSWB 语义查表加插值**（O(1)，不做逐行扫描），
//! 超限降级单 plane。**模式切换走原子序：先关后配再开**——中间态不可见
//! 由「可见使能只在配置全通过之后发生」结构性保证，任一步失败回滚快照。
//!
//! # 头注要点（每条都是判据的反面，写在这里供判据引用）
//!
//! ## 要点一：pipe 层级是**固定拓扑**，不是自由组合
//!
//! 每 pipe 恰三个 plane 槽（Primary/SPR/CUR），槽位有身份——把光标配到
//! 主槽、把精灵配到光标槽都是拓扑错误（`PlaneSlotMismatch`）。pipe 数
//! 上限由 F0221 能力探针对接（`display_pipes`），越界 pipe id 拒绝。
//!
//! ## 要点二：时序匹配是**带宽账**，link rate/lanes/时钟三要素合验
//!
//! 可用带宽 = link_rate × lanes × 编码效率（8b/10b = 80%）；需求带宽 =
//! mode_clock × 24bpp ÷ 8。需求 > 可用 ⇒ `TimingBandwidthMismatch`
//! 拒绝并**回落安全模式**（锚点降级矩阵原文）。非法参数值（lane 不在
//! 1/2/4、link rate 不在档位表内）在带宽账之前就按
//! `TimingIllegal` 拒——两码分开，错误面精确。
//!
//! ## 要点三：watermark 是**查表加插值**，超限降级单 plane
//!
//! DSWB 语义：活跃像素数查档位表，档间线性插值，结果封顶。watermark
//! 超过 [`WM_MAX`] ⇒ 多 plane 配置降级为**单 plane**（降级可观测记账
//! `wm_downgrades`），不静默放行超限值（超限值进硬件 = display FIFO
//! underrun）。
//!
//! ## 要点四：原子序是**先关后配再开**的固定三步，失败回滚快照
//!
//! 提交前快照 pipe 状态；序 = 关可见 → 配（时序+planes+watermark，全部
//! 校验）→ 开可见。配置步任一失败 ⇒ 回滚快照（回到提交前可见状态），
//! 返回**三要素通知**（什么坏了/影响/建议——锚点无障碍面：模式切换失败
//! 通知走三要素）。步骤计数确定性（判据断一帧内完成 = 步数 ≤
//! [`ATOMIC_STEP_BUDGET`]）。
//!
//! ## 要点五：Underrun 按**事件计数升级处置**，不是记一笔就完
//!
//! 每 pipe 独立计数：第 1 次记录；第 2 次告警升级；第 3 次起**停用肇事
//! pipe**（要求重新原子提交）——计数不升级 = underrun 风暴被日志淹没。
//! 处置等级与计数判据侧独立重算。
//!
//! ## 要点六：诊断码独占 0x35xx 段
//!
//! 与 F0222（0x2Fxx）/F0223（0x32xx）/F0224（0x34xx）互不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::veb21_ident::GenTier;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 每 pipe 的 plane 槽数（Primary/SPR/CUR，要点一：固定拓扑）。
pub const PLANES_PER_PIPE: usize = 3;

/// 编码效率（8b/10b = 80%，时序带宽账口径）。
pub const ENC_EFF_PCT: u32 = 80;

/// 24bpp ÷ 8 = 每像素字节数（带宽账口径）。
pub const BYTES_PER_PIXEL: u32 = 3;

/// DSWB watermark 上限（块数；**未封顶原值超限 ⇒ 降级单 plane**。
/// 档位表顶值 128 > 112：4K 级多 plane 配置触发降级——判据可达性）。
pub const WM_MAX: u16 = 112;

/// 原子序步骤预算（要点四：一帧内完成的确定性口径）。
pub const ATOMIC_STEP_BUDGET: u32 = 8;

/// Underrun 升级处置阈值（要点五：第 3 次起停用肇事 pipe）。
pub const UNDERRUN_DISABLE_AT: u32 = 3;

/// 安全模式时序（640×480@60，保守档；时序非法回落用）。
pub const SAFE_MODE_CLOCK_KHZ: u32 = 25175;
/// 安全模式 link rate（DP 1.62G 档）。
pub const SAFE_MODE_LINK_MHZ: u32 = 1620;
/// 安全模式 lane 数。
pub const SAFE_MODE_LANES: u8 = 2;

// ---------------------------------------------------------------------------
// 诊断码（0x35xx 独占段）
// ---------------------------------------------------------------------------

/// 显示控制域错误（**自建诊断码**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DErr {
    /// pipe id 越能力上限（F0221 探针口径）。
    BadPipe,
    /// plane 槽位身份不符（光标进主槽等拓扑错误）。
    PlaneSlotMismatch,
    /// 时序参数值非法（lane/link rate 不在档位表）。
    TimingIllegal,
    /// 时序带宽账不匹配（需求 > 可用）。
    TimingBandwidthMismatch,
    /// 原子提交校验失败（回滚已执行）。
    AtomicRolledBack,
    /// plane 尺寸/stride 非法。
    BadPlaneGeom,
    /// watermark 超限已降级（不是错误，是可观测降级事件）。
    WmDowngraded,
    /// Underrun 达停用阈值（pipe 已停用）。
    UnderrunDisabled,
    /// pipe 已被 Underrun 处置停用，先恢复再提交。
    PipeDisabled,
}

impl DErr {
    pub const fn code(self) -> u32 {
        match self {
            DErr::BadPipe => 0x3501,
            DErr::PlaneSlotMismatch => 0x3502,
            DErr::TimingIllegal => 0x3503,
            DErr::TimingBandwidthMismatch => 0x3504,
            DErr::AtomicRolledBack => 0x3505,
            DErr::BadPlaneGeom => 0x3506,
            DErr::WmDowngraded => 0x3507,
            DErr::UnderrunDisabled => 0x3508,
            DErr::PipeDisabled => 0x3509,
        }
    }
    /// 专属 reason（不共用占位串）。
    pub fn reason(self) -> String {
        match self {
            DErr::BadPipe => String::from("pipe id 越能力上限"),
            DErr::PlaneSlotMismatch => String::from("plane 槽位身份不符"),
            DErr::TimingIllegal => String::from("时序参数值非法"),
            DErr::TimingBandwidthMismatch => String::from("时序带宽账不匹配"),
            DErr::AtomicRolledBack => String::from("原子提交失败已回滚快照"),
            DErr::BadPlaneGeom => String::from("plane 尺寸或 stride 非法"),
            DErr::WmDowngraded => String::from("watermark 超限已降级单 plane"),
            DErr::UnderrunDisabled => String::from("underrun 达阈值 pipe 已停用"),
            DErr::PipeDisabled => String::from("pipe 处于停用态先恢复再提交"),
        }
    }
    pub const ALL: [DErr; 9] = [
        DErr::BadPipe,
        DErr::PlaneSlotMismatch,
        DErr::TimingIllegal,
        DErr::TimingBandwidthMismatch,
        DErr::AtomicRolledBack,
        DErr::BadPlaneGeom,
        DErr::WmDowngraded,
        DErr::UnderrunDisabled,
        DErr::PipeDisabled,
    ];
}

/// 三要素通知（锚点无障碍面：模式切换失败通知走三要素；
/// 什么坏了 / 影响 / 建议——三件全非空才构成有效通知）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwitchNotice {
    pub what: &'static str,
    pub impact: &'static str,
    pub action: &'static str,
}

// ---------------------------------------------------------------------------
// 代际能力对接（上游 F0221）
// ---------------------------------------------------------------------------

/// 显示能力（由 [`GenTier`] 推导：基线档 link rate 上限 5400，Xe 起支持
/// 8100 档；pipe 数上限恒 3——Gen9/Xe 均 3 管线）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DispCaps {
    pub max_pipes: u8,
    /// 最高 link rate 档（MHz）。
    pub max_link_mhz: u32,
}

impl DispCaps {
    pub const fn for_tier(t: GenTier) -> DispCaps {
        match t {
            GenTier::Baseline => DispCaps { max_pipes: 3, max_link_mhz: 5400 },
            GenTier::XeStandard | GenTier::XeLatest => DispCaps { max_pipes: 3, max_link_mhz: 8100 },
        }
    }
}

// ---------------------------------------------------------------------------
// 时序参数与带宽账（要点二）
// ---------------------------------------------------------------------------

/// 时序参数（时钟×lane×link rate 三要素）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimingParams {
    /// 模式时钟（kHz）。
    pub mode_clock_khz: u32,
    /// link rate（MHz；档位 1620/2700/5400/8100）。
    pub link_mhz: u32,
    /// lane 数（1/2/4）。
    pub lanes: u8,
    /// 水平有效像素（带宽账的活跃宽度口径）。
    pub hdisplay: u16,
    /// 垂直有效行。
    pub vdisplay: u16,
    /// 刷新率（Hz）。
    pub vrefresh: u8,
}

/// link rate 档位表（判据侧字面量同源对拍）。
pub const LINK_RATES: [u32; 4] = [1620, 2700, 5400, 8100];

impl TimingParams {
    /// 参数值合法性（带宽账之前的第一道：档位表内才放行）。
    pub fn value_legal(&self) -> bool {
        let mut rate_ok = false;
        let mut i = 0usize;
        while i < LINK_RATES.len() {
            if LINK_RATES[i] == self.link_mhz {
                rate_ok = true;
            }
            i += 1;
        }
        let lanes_ok = self.lanes == 1 || self.lanes == 2 || self.lanes == 4;
        rate_ok && lanes_ok && self.mode_clock_khz > 0 && self.hdisplay > 0 && self.vdisplay > 0
            && self.vrefresh > 0
    }

    /// 需求带宽（kB/s 口径：clock × bpp/8，clock 已含消隐的等效像素率）。
    pub fn required_kb(&self) -> u64 {
        (self.mode_clock_khz as u64) * (BYTES_PER_PIXEL as u64)
    }

    /// 可用带宽（kB/s 口径：link × lanes × 80% 编码效率）。
    pub fn available_kb(&self) -> u64 {
        (self.link_mhz as u64) * 1000 * (self.lanes as u64) * (ENC_EFF_PCT as u64) / 100
    }

    /// 带宽匹配（需求 ≤ 可用）。
    pub fn bandwidth_ok(&self) -> bool {
        self.required_kb() <= self.available_kb()
    }
}

// ---------------------------------------------------------------------------
// watermark（DSWB 语义，要点三：查表加插值）
// ---------------------------------------------------------------------------

/// DSWB 档位表：（活跃像素数下界, watermark 块数）——升序排列。
pub const WM_TABLE: [(u32, u16); 6] = [
    (0, 8),
    (512, 16),
    (1024, 32),
    (2048, 64),
    (3840, 96),
    (7680, 128),
];

/// O(1) 查表加插值：活跃像素 → watermark 块数（**未封顶原值**——
/// 超限判定与降级判定都吃这个原值；判据侧独立重算同口径）。
pub fn wm_compute_raw(active_px: u32) -> u32 {
    let n = WM_TABLE.len();
    if active_px <= WM_TABLE[0].0 {
        return WM_TABLE[0].1 as u32;
    }
    if active_px >= WM_TABLE[n - 1].0 {
        return WM_TABLE[n - 1].1 as u32;
    }
    // 档位定位（表长 6 = O(1) 量级；线性步进有界）。
    let mut k = 0usize;
    while k + 1 < n && active_px > WM_TABLE[k + 1].0 {
        k += 1;
    }
    let (x0, y0) = WM_TABLE[k];
    let (x1, y1) = WM_TABLE[k + 1];
    // 线性插值（分母恒正：x1 > x0 由表不变式保证）。
    let span = (x1 - x0) as u64;
    let frac = (active_px - x0) as u64 * 1000 / span;
    ((y0 as u64) + ((y1 as u64 - y0 as u64) * frac) / 1000) as u32
}

/// O(1) 查表加插值（封顶 [`WM_MAX`] 的最终生效值）。
pub fn wm_compute(active_px: u32) -> u16 {
    let v = wm_compute_raw(active_px);
    if v > WM_MAX as u32 {
        WM_MAX
    } else {
        v as u16
    }
}

// ---------------------------------------------------------------------------
// pipe/plane 状态（要点一：固定拓扑）
// ---------------------------------------------------------------------------

/// pipe 标识（0..caps.max_pipes）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PipeId(pub u8);

/// plane 槽身份（Primary=0 / SPR=1 / CUR=2，固定序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlaneKind {
    /// 主 plane。
    Primary,
    /// 精灵 plane。
    Sprite,
    /// 光标 plane。
    Cursor,
}

impl PlaneKind {
    pub const fn slot(self) -> usize {
        match self {
            PlaneKind::Primary => 0,
            PlaneKind::Sprite => 1,
            PlaneKind::Cursor => 2,
        }
    }
}

/// plane 配置（尺寸/stride/格式；表面地址在 aperture 内属 F0222 域，
/// 本层只承载几何与格式——地址零暴露）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PlaneCfg {
    pub kind: PlaneKind,
    pub width: u16,
    pub height: u16,
    /// stride（字节；须 ≥ width×4）。
    pub stride: u32,
    /// 格式码（0=XRGB8888 1=ARGB8888 2=RGB565，判据侧字面量）。
    pub fmt: u8,
}

impl PlaneCfg {
    fn geom_ok(&self) -> bool {
        self.width > 0 && self.height > 0 && (self.stride as u64) >= (self.width as u64) * 4
            && self.fmt <= 2
    }
}

/// pipe 状态（可见性 + 时序 + 三 plane 槽）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PipeState {
    pub visible: bool,
    pub timing: Option<TimingParams>,
    pub planes: [Option<PlaneCfg>; PLANES_PER_PIPE],
    /// 生效 watermark（最后一笔原子提交计算值）。
    pub wm: u16,
}

impl PipeState {
    /// 空白 pipe 态（判据侧兜底读用；零 panic 面配套）。
    pub const fn blank_state() -> PipeState {
        PipeState { visible: false, timing: None, planes: [None; PLANES_PER_PIPE], wm: 0 }
    }
}

// ---------------------------------------------------------------------------
// 显示控制器总控
// ---------------------------------------------------------------------------

/// 提交账本（判据侧独立重算对拍）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DispStats {
    pub atomic_commits: u32,
    pub atomic_rollbacks: u32,
    pub safe_mode_falls: u32,
    pub wm_downgrades: u32,
    pub underruns: u32,
    pub pipes_disabled: u32,
    pub steps_last_commit: u32,
}

impl DispStats {
    pub const fn zero() -> DispStats {
        DispStats {
            atomic_commits: 0,
            atomic_rollbacks: 0,
            safe_mode_falls: 0,
            wm_downgrades: 0,
            underruns: 0,
            pipes_disabled: 0,
            steps_last_commit: 0,
        }
    }
}

/// Intel 显示控制器（pipe/plane 总控）。
pub struct DisplayCtl {
    caps: DispCaps,
    pipes: Vec<PipeState>,
    /// 每 pipe 的 underrun 事件计数（升级处置账）。
    underrun_counts: [u32; 3],
    /// 每 pipe 停用态。
    disabled: [bool; 3],
    /// 最近一次切换失败的三要素通知。
    pub last_notice: Option<SwitchNotice>,
    /// 最近一笔原子提交是否触发了 watermark 降级（可观测降级账）。
    pub wm_downgrade_last: bool,
    pub stats: DispStats,
}

impl DisplayCtl {
    /// 构造（pipe 数由 F0221 探针结果给定，越能力上限即拒——探针说几个
    /// 就是几个，本层不放大）。
    pub fn new(tier: GenTier, probe_pipes: u8) -> Result<DisplayCtl, DErr> {
        let caps = DispCaps::for_tier(tier);
        if probe_pipes == 0 || probe_pipes > caps.max_pipes {
            return Err(DErr::BadPipe);
        }
        let mut pipes: Vec<PipeState> = Vec::new();
        let mut i = 0u8;
        while i < probe_pipes {
            pipes.push(PipeState::blank_state());
            i += 1;
        }
        Ok(DisplayCtl {
            caps,
            pipes,
            underrun_counts: [0; 3],
            disabled: [false; 3],
            last_notice: None,
            wm_downgrade_last: false,
            stats: DispStats::zero(),
        })
    }

    pub fn caps(&self) -> DispCaps {
        self.caps
    }

    pub fn pipe_count(&self) -> u8 {
        self.pipes.len() as u8
    }

    /// 读 pipe 状态（判据对拍用）。
    pub fn pipe(&self, p: PipeId) -> Option<PipeState> {
        if (p.0 as usize) < self.pipes.len() {
            Some(self.pipes[p.0 as usize])
        } else {
            None
        }
    }

    // —— 原子序提交（要点四：先关后配再开）——

    /// 原子提交：快照 → 关可见 → 配（时序带宽账 + planes 拓扑/几何 +
    /// watermark）→ 开可见；任一失败回滚快照并给三要素通知。
    pub fn atomic_commit(
        &mut self,
        p: PipeId,
        timing: TimingParams,
        planes: [Option<PlaneCfg>; PLANES_PER_PIPE],
    ) -> Result<(), DErr> {
        if (p.0 as usize) >= self.pipes.len() {
            return Err(DErr::BadPipe);
        }
        if self.disabled[p.0 as usize] {
            return Err(DErr::PipeDisabled);
        }
        let mut steps = 0u32;
        // 快照（回滚基准）。
        let snap = self.pipes[p.0 as usize];
        // 步 1：关可见（中间态不可见的结构性起点）。
        self.pipes[p.0 as usize].visible = false;
        steps += 1;
        // 步 2：配置——先值合法，再带宽账，再 plane 拓扑与几何。
        if !timing.value_legal() {
            return self.rollback(p, snap, steps, DErr::TimingIllegal,
                "时序参数值非法", "模式未切换保持原样", "改用档位表内的 link rate 与 1/2/4 lane");
        }
        if !timing.bandwidth_ok() {
            return self.rollback(p, snap, steps, DErr::TimingBandwidthMismatch,
                "时序带宽不匹配", "模式未切换保持原样", "提高 link rate 或 lane 数后再提交");
        }
        let mut si = 0usize;
        while si < PLANES_PER_PIPE {
            if let Some(c) = planes[si] {
                if c.kind.slot() != si {
                    return self.rollback(p, snap, steps, DErr::PlaneSlotMismatch,
                        "plane 槽位身份不符", "该 plane 不生效", "按 Primary/SPR/CUR 槽位重新提交");
                }
                if !c.geom_ok() {
                    return self.rollback(p, snap, steps, DErr::BadPlaneGeom,
                        "plane 几何非法", "该 plane 不生效", "核对 width/height/stride 后重新提交");
                }
            }
            si += 1;
        }
        // watermark（主 plane 活跃像素口径；**未封顶原值超限 ⇒ 降级
        // 单 plane**——锚点降级矩阵原文，降级可观测记账不静默）。
        let active_px = match planes[PlaneKind::Primary.slot()] {
            Some(c) => c.width as u32 * c.height as u32,
            None => {
                // 无主 plane：取第一个存在的 plane 的像素口径。
                let mut px = 0u32;
                let mut qi = 0usize;
                while qi < PLANES_PER_PIPE {
                    if let Some(c) = planes[qi] {
                        px = c.width as u32 * c.height as u32;
                        qi = PLANES_PER_PIPE;
                    } else {
                        qi += 1;
                    }
                }
                px
            }
        };
        let raw_wm = wm_compute_raw(active_px);
        let mut eff_planes = planes;
        let mut wm_final = wm_compute(active_px);
        if raw_wm > WM_MAX as u32 {
            // 降级单 plane：只保留第一个存在的 plane，其余清空。
            self.stats.wm_downgrades += 1;
            self.wm_downgrade_last = true;
            wm_final = WM_MAX;
            let mut single: [Option<PlaneCfg>; PLANES_PER_PIPE] = [None; PLANES_PER_PIPE];
            let mut kept = false;
            let mut di = 0usize;
            while di < PLANES_PER_PIPE {
                if !kept {
                    if let Some(c) = planes[di] {
                        single[di] = Some(c);
                        kept = true;
                    }
                }
                di += 1;
            }
            eff_planes = single;
        } else {
            self.wm_downgrade_last = false;
        }
        // 配置落笔（校验全过才写）。
        self.pipes[p.0 as usize].timing = Some(timing);
        self.pipes[p.0 as usize].planes = eff_planes;
        self.pipes[p.0 as usize].wm = wm_final;
        steps += 1;
        // 步 3：开可见（配置全通过之后才发生——中间态不可见的结构性终点）。
        self.pipes[p.0 as usize].visible = true;
        steps += 1;
        self.stats.steps_last_commit = steps;
        self.stats.atomic_commits += 1;
        Ok(())
    }

    /// 回滚快照 + 三要素通知（私有；步骤如实入账）。
    fn rollback(
        &mut self,
        p: PipeId,
        snap: PipeState,
        steps: u32,
        e: DErr,
        what: &'static str,
        impact: &'static str,
        action: &'static str,
    ) -> Result<(), DErr> {
        self.pipes[p.0 as usize] = snap;
        self.stats.steps_last_commit = steps + 1; // 回滚步计入
        self.stats.atomic_rollbacks += 1;
        self.last_notice = Some(SwitchNotice { what, impact, action });
        Err(e)
    }

    // —— 时序非法回落安全模式（锚点降级矩阵）——

    /// 安全模式时序（保守档常量；判据侧字面量同源对拍）。
    pub fn safe_mode() -> TimingParams {
        TimingParams {
            mode_clock_khz: SAFE_MODE_CLOCK_KHZ,
            link_mhz: SAFE_MODE_LINK_MHZ,
            lanes: SAFE_MODE_LANES,
            hdisplay: 640,
            vdisplay: 480,
            vrefresh: 60,
        }
    }

    /// 时序非法时的回落路径：以安全模式重试一次原子提交。
    pub fn commit_or_safe(
        &mut self,
        p: PipeId,
        timing: TimingParams,
        planes: [Option<PlaneCfg>; PLANES_PER_PIPE],
    ) -> Result<bool, DErr> {
        match self.atomic_commit(p, timing, planes) {
            Ok(()) => Ok(false),
            Err(DErr::TimingIllegal) | Err(DErr::TimingBandwidthMismatch) => {
                self.stats.safe_mode_falls += 1;
                self.atomic_commit(p, Self::safe_mode(), planes)?;
                Ok(true)
            }
            Err(e) => Err(e),
        }
    }

    // —— Underrun 升级处置（要点五）——

    /// Underrun 中断入口：计数升级处置，返回当前处置等级
    /// （1=记录 2=告警 3+=停用）。
    pub fn underrun_irq(&mut self, p: PipeId) -> Result<u32, DErr> {
        if (p.0 as usize) >= self.pipes.len() {
            return Err(DErr::BadPipe);
        }
        self.underrun_counts[p.0 as usize] += 1;
        self.stats.underruns += 1;
        let n = self.underrun_counts[p.0 as usize];
        if n >= UNDERRUN_DISABLE_AT && !self.disabled[p.0 as usize] {
            // 停用肇事 pipe（要求重新原子提交前先恢复）。
            self.disabled[p.0 as usize] = true;
            self.pipes[p.0 as usize].visible = false;
            self.stats.pipes_disabled += 1;
            Ok(n)
        } else {
            Ok(n)
        }
    }

    /// 恢复被 Underrun 处置停用的 pipe（重新启用前清计数——处置升级
    /// 账随恢复清零，underrun 风暴在恢复后重新从 1 数起）。
    pub fn reenable(&mut self, p: PipeId) -> Result<(), DErr> {
        if (p.0 as usize) >= self.pipes.len() {
            return Err(DErr::BadPipe);
        }
        self.disabled[p.0 as usize] = false;
        self.underrun_counts[p.0 as usize] = 0;
        Ok(())
    }

    pub fn is_disabled(&self, p: PipeId) -> bool {
        (p.0 as usize) < 3 && self.disabled[p.0 as usize]
    }

    pub fn underrun_count(&self, p: PipeId) -> u32 {
        if (p.0 as usize) < 3 {
            self.underrun_counts[p.0 as usize]
        } else {
            0
        }
    }

    // —— 读屏摘要（聚合口径，不含表面地址——隐私红线）——

    pub fn status_summary(&self) -> String {
        let mut s = String::from("显示控制：管道 ");
        s.push_str(&self.pipe_count().to_string());
        s.push_str(" 条；原子提交 ");
        s.push_str(&self.stats.atomic_commits.to_string());
        s.push_str(" 次回滚 ");
        s.push_str(&self.stats.atomic_rollbacks.to_string());
        s.push_str(" 次；安全模式回落 ");
        s.push_str(&self.stats.safe_mode_falls.to_string());
        s.push_str(" 次；underrun ");
        s.push_str(&self.stats.underruns.to_string());
        s.push_str(" 次");
        s
    }
}
