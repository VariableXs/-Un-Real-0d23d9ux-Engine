//! VE-F0047 · 多缓冲策略自适应（VE-A 域 · A03 同步与呈现组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0047`
//!
//! **职责定位（锚点原文）**：多缓冲策略自适应——双/三缓冲的自适应选择
//! （延迟与吞吐的取舍表驱动），缓冲数变更的平滑过渡，策略与 VRR 的联动；
//! 含缓冲策略的用户覆盖（自动推荐之上允许手选）。数据结构：策略器；过渡器。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! - 切换失败 → **回退**（[`BufferTransition`] 的回退相：任一失败都不改现役
//!   缓冲数，旧策略继续生效，失败留痕）；
//! - 策略失配 → **建议**（现役缓冲数与推荐连续 [`MISMATCH_STRIKES`] 次不符
//!   才出一次建议——不强制切换，建议是建议，命令是命令）；
//! - VRR 冲突 → **仲裁**（VRR 生效时立即呈现与之冲突，仲裁保 VRR：立即请求
//!   被挡且计数，不静默放行也不静默改档）。
//!
//! **性能逐项分解**：O(1)——评估是查表（2×2 定容取舍表）+ 常数条规则，
//! 无历史扫描；过渡器每步 O(1)。无任何随帧数增长的状态。
//!
//! **跨批对接点**：A48 同步协同——F0048 管同步机制（VSync 档位/邮箱），
//! 本条只消费其结论（[`SyncDemand`]），不选机制；邮箱类的三缓冲语义下界
//! 与 F0046 的 `min_buffers(Mailbox)=3` 对齐（本条自持常量，不跨模块 use，
//! 见文末自持说明）。
//!
//! **无障碍与隐私**：策略状态读屏可达（[`PolicyAdvisor::a11y_lines`]）——
//! 中英双语七行，只报聚合计数与策略事实，不报窗口标题、不泄漏单帧延迟。
//!
//! ## 设计要点（为什么这样写）
//!
//! - **取舍表是数据不是 if 链**（[`TRADEOFF_TABLE`]）：延迟紧/松 × GPU
//!   忙/闲四格各有明确结论与理由码。拍成 if 链之后，「表驱动」就只剩
//!   注释里的三个字，改一格阈值要读全部分支。
//! - **建议与切换严格分离**：策略失配只累积「失配计数」并出建议
//!   （[`MISMATCH_STRIKES`] 次连续失配计一次），绝不借建议之手改现役
//!   缓冲数。现役链的变更只有一条路：[`BufferTransition`] 的平滑过渡。
//! - **平滑过渡 = 静默点应用**（[`BufferTransition`]）：缓冲数变更必须
//!   等「无飞帧引用」这一静默点才应用——带着在飞的帧硬换缓冲数等于
//!   让飞帧引用不存在的资源。排空超预算或应用注入失败 ⇒ 回退旧档，
//!   现役缓冲数不变（锚点「切换失败→回退」）。
//! - **VRR 仲裁有裁决值**（[`VrrArbitration`]）：冲突不是 bool 而是
//!   四值裁决（无冲突/挡立即/因VRR降档/窄窗视作定频），裁决可查可计数
//!   ——「仲裁过」三个字没法对账。
//! - **用户覆盖是覆盖，不是重置**（[`Override`]）：手选合法档（双/三）
//!   即生效于自动推荐之上；档外值拒绝并计数、保留原覆盖。手选与同步
//!   地板冲突时不拦（用户主权），但冲突可见（计数）。
//! - **零 panic 面**：查表下标由枚举 `ordinal()` 生成（定容数组内）、
//!   帧时间比较走 u64 防溢出、无 unwrap/expect/切片越界。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F0046** 管「选定后缓冲数与描述符必须一致、重建必须原子」；本条
//!   管「缓冲数怎么选、怎么平滑换」。本条的输出（最终档）喂给 F0046
//!   的重建事务。
//! - **F0048** 管同步机制；本条只把「邮箱类同步」当作三缓冲语义地板
//!   （两缓冲下邮箱退化为保序，见 F0046 同名判据）。
//! - **F0050** 管丢帧归因；本条只把「最近丢帧数」当作升级信号消费，
//!   不归因。
//!
//! ## 自持说明
//!
//! 本条自持全部类型与常量，不跨模块 `use` svstar2 内其他模块——并行提交
//! 时跨模块引用会把两个模块的编译成败绑在一起，一方半成品就拖垮另一方
//! （E0583 与真实缺陷难以分辨）。邮箱地板常量与 F0046 的语义对齐是
//! **约定**而非**引用**，判据侧独立钉住两侧数值一致的方向。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本条策略作用域：双缓冲。
pub const BUFFERS_DOUBLE: u8 = 2;

/// 本条策略作用域：三缓冲。锚点明文「双/三缓冲的自适应选择」——
/// 单缓冲不在本条自适应范围内（单缓冲无可缓冲余量可言）。
pub const BUFFERS_TRIPLE: u8 = 3;

/// GPU 忙判定阈值（占用百分比 ≥ 此值即 Busy 档）。
pub const GPU_BUSY_PCT: u8 = 85;

/// 帧耗时逼近刷新周期的判定分母（帧耗时×10 ≥ 周期×9 即 ≥90% ⇒ Busy）。
pub const BUSY_RATIO_DEN: u64 = 10;

/// 帧耗时逼近刷新周期的判定分子（对应 90%）。
pub const BUSY_RATIO_NUM: u64 = 9;

/// 最近丢帧数达到此值即升级一档（双→三）：丢帧是吞吐饥饿的直接证据。
pub const DROPS_ESCALATE_AT: u8 = 2;

/// 策略失配连续计到此次数才出一次建议——低于此值频繁切档比失配本身更伤。
pub const MISMATCH_STRIKES: usize = 4;

/// 过渡排空预算（步数）：超过即判定排空卡死，回退旧档。
pub const DRAIN_BUDGET_STEPS: usize = 16;

/// VRR 窄窗阈值：max−min 低于此跨度（Hz）的 VRR 视作无效，按定频处理。
pub const VRR_NARROW_HZ: u16 = 10;

/// 邮箱类同步的三缓冲语义地板（与 F0046 `min_buffers(Mailbox)=3` 对齐的
/// 约定值，判据独立钉住）。
pub const SYNC_MAILBOX_MIN_BUFFERS: u8 = BUFFERS_TRIPLE;

/// 读屏面板行数。
pub const PANEL_LINES: usize = 7;

// ---------------------------------------------------------------------------
// 二、诊断码（自建，新域独占 0x47xx 段）
// ---------------------------------------------------------------------------

/// 呈现策略诊断码类型。
pub type PolicyCode = u16;

/// 码段: 遥测非法（零帧耗时/零刷新周期/GPU 占用越界）。
pub const CODE_BAD_TELEMETRY: PolicyCode = 0x4701;

/// 码段: 用户覆盖被拒（档外值）。
pub const CODE_OVERRIDE_REJECTED: PolicyCode = 0x4702;

/// 码段: VRR 频率范围非法（min=0 或 min>max）。
pub const CODE_VRR_BAD_RANGE: PolicyCode = 0x4703;

/// 码段: VRR 与立即呈现冲突（仲裁裁决码，供 A48 接缝消费）。
pub const CODE_VRR_CONFLICT: PolicyCode = 0x4704;

/// 码段: 过渡排空超预算（切换失败→回退）。
pub const CODE_DRAIN_TIMEOUT: PolicyCode = 0x4705;

/// 码段: 过渡应用段注入故障（演练）。
pub const CODE_APPLY_FAULT: PolicyCode = 0x4706;

/// 码段: 非静默点强行应用（仍有飞帧引用旧缓冲）。
pub const CODE_APPLY_NOT_QUIESCENT: PolicyCode = 0x4707;

/// 码段: 读屏面板行数与实际不符。
pub const CODE_PANEL_SHAPE: PolicyCode = 0x4708;

/// 码段: 过渡计划的起止档非法（双/三之外）。
pub const CODE_BAD_BUFFER_COUNT: PolicyCode = 0x4709;

// ---------------------------------------------------------------------------
// 三、输入：遥测快照与同步档需求
// ---------------------------------------------------------------------------

/// 同步档需求（来自 A48/F0048 的结论，本条只消费不裁决机制）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncDemand {
    /// 保序类（fifo / VSync 开）。
    FifoLike,
    /// 邮箱类（低延迟且不撕裂的承诺 ⇒ 三缓冲语义地板）。
    MailboxLike,
    /// 立即类（最低延迟，允许撕裂；与生效中的 VRR 冲突）。
    ImmediateLike,
}

impl SyncDemand {
    pub const ALL: [SyncDemand; 3] =
        [SyncDemand::FifoLike, SyncDemand::MailboxLike, SyncDemand::ImmediateLike];

    pub const fn ordinal(self) -> usize {
        match self {
            SyncDemand::FifoLike => 0,
            SyncDemand::MailboxLike => 1,
            SyncDemand::ImmediateLike => 2,
        }
    }

    pub const fn wire(self) -> u8 {
        self.ordinal() as u8
    }

    pub const fn from_wire(w: u8) -> Option<SyncDemand> {
        if (w as usize) < 3 {
            Some(SyncDemand::ALL[w as usize])
        } else {
            None
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            SyncDemand::FifoLike => "保序类 / fifo-like",
            SyncDemand::MailboxLike => "邮箱类 / mailbox-like",
            SyncDemand::ImmediateLike => "立即类 / immediate-like",
        }
    }

    /// 本档需求的缓冲数语义地板：邮箱类要三缓冲（两缓冲下邮箱退化为
    /// 保序，拿不到「不撕裂且低延迟」同时成立），其余不设地板。
    pub const fn floor_buffers(self) -> u8 {
        match self {
            SyncDemand::MailboxLike => SYNC_MAILBOX_MIN_BUFFERS,
            SyncDemand::FifoLike => 0,
            SyncDemand::ImmediateLike => 0,
        }
    }
}

/// 一帧遥测快照（评估的唯一输入，O(1) 可得）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Telemetry {
    /// 呈现延迟 P95（微秒）。
    pub latency_p95_us: u32,
    /// 最近帧耗时（微秒）。
    pub frame_time_us: u32,
    /// 刷新周期（微秒；1e6 / 刷新率）。
    pub refresh_period_us: u32,
    /// GPU 占用百分比（0..=100）。
    pub gpu_busy_pct: u8,
    /// 最近窗口丢帧数。
    pub dropped_recent: u8,
    /// VRR 是否启用。
    pub vrr_enabled: bool,
    /// VRR 最小刷新率（Hz）。
    pub vrr_min_hz: u16,
    /// VRR 最大刷新率（Hz）。
    pub vrr_max_hz: u16,
    /// 同步档需求（A48 结论）。
    pub sync: SyncDemand,
}

impl Telemetry {
    pub const fn new(
        latency_p95_us: u32,
        frame_time_us: u32,
        refresh_period_us: u32,
        gpu_busy_pct: u8,
        dropped_recent: u8,
        vrr_enabled: bool,
        vrr_min_hz: u16,
        vrr_max_hz: u16,
        sync: SyncDemand,
    ) -> Telemetry {
        Telemetry {
            latency_p95_us,
            frame_time_us,
            refresh_period_us,
            gpu_busy_pct,
            dropped_recent,
            vrr_enabled,
            vrr_min_hz,
            vrr_max_hz,
            sync,
        }
    }

    /// 遥测自检（边界防护的第一道闸）。
    ///
    /// 零帧耗时/零刷新周期不是「极快」而是「没测到」——拿没测到的值查表
    /// 等于用假数据选档。VRR 启用时范围必须合法（min ≥ 1 且 min ≤ max）。
    pub const fn validate(&self) -> Result<(), PolicyCode> {
        if self.frame_time_us == 0 || self.refresh_period_us == 0 {
            return Err(CODE_BAD_TELEMETRY);
        }
        if self.gpu_busy_pct > 100 {
            return Err(CODE_BAD_TELEMETRY);
        }
        if self.vrr_enabled && (self.vrr_min_hz == 0 || self.vrr_min_hz > self.vrr_max_hz) {
            return Err(CODE_VRR_BAD_RANGE);
        }
        Ok(())
    }

    /// VRR 频率跨度（未启用或非法时为 0）。
    pub const fn vrr_span_hz(&self) -> u16 {
        if !self.vrr_enabled || self.vrr_min_hz > self.vrr_max_hz {
            0
        } else {
            self.vrr_max_hz - self.vrr_min_hz
        }
    }

    /// 延迟档：P95 不高于预算即 Tight（**恰好等于预算也算紧**——预算就是
    /// 上界，等于上界没有余量可言；写成 `<` 会把贴线的延迟误判为宽松）。
    pub const fn latency_class(&self, budget_us: u32) -> LatencyClass {
        if self.latency_p95_us <= budget_us {
            LatencyClass::Tight
        } else {
            LatencyClass::Loose
        }
    }

    /// 余量档：GPU 占用达标**或**帧耗时逼近刷新周期（u64 乘法防溢出）。
    pub const fn headroom_class(&self) -> Headroom {
        if self.gpu_busy_pct >= GPU_BUSY_PCT {
            return Headroom::Busy;
        }
        let ft = self.frame_time_us as u64;
        let rp = self.refresh_period_us as u64;
        if ft * BUSY_RATIO_DEN >= rp * BUSY_RATIO_NUM {
            Headroom::Busy
        } else {
            Headroom::Idle
        }
    }
}

/// 延迟档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatencyClass {
    /// P95 不高于延迟预算（预算即上界）。
    Tight,
    /// P95 高于预算。
    Loose,
}

impl LatencyClass {
    pub const fn ordinal(self) -> usize {
        match self {
            LatencyClass::Tight => 0,
            LatencyClass::Loose => 1,
        }
    }
}

/// 余量档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Headroom {
    /// GPU 忙（占用达标或帧耗时贴刷新周期）。
    Busy,
    /// GPU 闲。
    Idle,
}

impl Headroom {
    pub const fn ordinal(self) -> usize {
        match self {
            Headroom::Busy => 0,
            Headroom::Idle => 1,
        }
    }
}

// ---------------------------------------------------------------------------
// 四、取舍表（表驱动：结论是数据，不是 if 链）
// ---------------------------------------------------------------------------

/// 推荐理由码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// 延迟紧且 GPU 闲：双缓冲队列最短。
    TableLatencyIdle,
    /// 延迟紧但 GPU 忙：仍取双缓冲保延迟，张力计数可见（吞吐压力交给
    /// 丢帧升级信号，不预支延迟）。
    TableTension,
    /// 延迟宽且 GPU 闲：双缓冲即可（没人需要第三块缓冲闲着）。
    TableIdle,
    /// 延迟宽且 GPU 忙：三缓冲吸收波动保吞吐。
    TableThroughput,
    /// 丢帧升级（双→三）：丢帧是吞吐饥饿的直接证据。
    DropEscalation,
    /// 邮箱类同步地板抬高到三缓冲。
    SyncFloorMailbox,
    /// VRR 生效使三缓冲的多余排队显形，降回双缓冲。
    VrrDemotion,
    /// 用户手选覆盖自动推荐。
    UserOverride,
}

impl Reason {
    pub const ALL_LEN: usize = 8;

    pub const fn ordinal(self) -> usize {
        match self {
            Reason::TableLatencyIdle => 0,
            Reason::TableTension => 1,
            Reason::TableIdle => 2,
            Reason::TableThroughput => 3,
            Reason::DropEscalation => 4,
            Reason::SyncFloorMailbox => 5,
            Reason::VrrDemotion => 6,
            Reason::UserOverride => 7,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Reason::TableLatencyIdle => "延迟紧GPU闲取双 / tight-latency idle picks double",
            Reason::TableTension => "延迟紧GPU忙张力 / tight-latency busy tension",
            Reason::TableIdle => "延迟宽GPU闲取双 / loose-latency idle picks double",
            Reason::TableThroughput => "延迟宽GPU忙取三 / loose-latency busy picks triple",
            Reason::DropEscalation => "丢帧升级三缓冲 / drop escalation to triple",
            Reason::SyncFloorMailbox => "邮箱地板三缓冲 / mailbox floor triple",
            Reason::VrrDemotion => "VRR降档双缓冲 / vrr demotion to double",
            Reason::UserOverride => "用户手选 / user override",
        }
    }
}

/// 取舍表一格：推荐档 + 理由。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableEntry {
    pub buffers: u8,
    pub reason: Reason,
}

/// 维度基数（2×2 定容表）。
pub const AXIS_KINDS: usize = 2;

/// 取舍表本体：`[延迟档][余量档]`。
///
/// - Tight×Idle：双缓冲（队列最短，余量足够不需要吸收波动）。
/// - Tight×Busy：双缓冲（保延迟优先；张力计数可见，吞吐压力走丢帧升级）。
/// - Loose×Idle：双缓冲（闲时三缓冲纯浪费显存与带宽）。
/// - Loose×Busy：三缓冲（唯一默认三缓冲格：忙且延迟预算宽松）。
pub const TRADEOFF_TABLE: [[TableEntry; AXIS_KINDS]; AXIS_KINDS] = [
    // [Tight][...]
    [
        TableEntry { buffers: BUFFERS_DOUBLE, reason: Reason::TableLatencyIdle },
        TableEntry { buffers: BUFFERS_DOUBLE, reason: Reason::TableTension },
    ],
    // [Loose][...]
    [
        TableEntry { buffers: BUFFERS_DOUBLE, reason: Reason::TableIdle },
        TableEntry { buffers: BUFFERS_TRIPLE, reason: Reason::TableThroughput },
    ],
];

/// 查表（O(1)：下标由枚举 ordinal 生成，恒在定容数组内，零 panic 面）。
pub const fn tradeoff_entry(lat: LatencyClass, head: Headroom) -> TableEntry {
    TRADEOFF_TABLE[lat.ordinal()][head.ordinal()]
}

// ---------------------------------------------------------------------------
// 五、VRR 联动与仲裁
// ---------------------------------------------------------------------------

/// VRR 仲裁裁决（冲突不是 bool，是可查可计数的四值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VrrArbitration {
    /// 无冲突、无降档。
    None,
    /// VRR 生效 × 立即类请求：仲裁保 VRR，立即请求被挡（附裁决码
    /// [`CODE_VRR_CONFLICT`] 供 A48 消费）。
    BlockedImmediate,
    /// VRR 生效使三缓冲的多余排队显形，自动降回双缓冲。
    DemotedForVrr,
    /// VRR 窄窗（跨度低于 [`VRR_NARROW_HZ`]）：视作定频，不参与联动。
    TreatedFixed,
}

impl VrrArbitration {
    pub const ALL_LEN: usize = 4;

    pub const fn ordinal(self) -> usize {
        match self {
            VrrArbitration::None => 0,
            VrrArbitration::BlockedImmediate => 1,
            VrrArbitration::DemotedForVrr => 2,
            VrrArbitration::TreatedFixed => 3,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            VrrArbitration::None => "无冲突 / no conflict",
            VrrArbitration::BlockedImmediate => "仲裁挡立即 / blocked immediate",
            VrrArbitration::DemotedForVrr => "因VRR降档 / demoted for vrr",
            VrrArbitration::TreatedFixed => "窄窗视作定频 / narrow window as fixed",
        }
    }
}

// ---------------------------------------------------------------------------
// 六、用户覆盖（自动推荐之上允许手选）
// ---------------------------------------------------------------------------

/// 缓冲策略覆盖。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Override {
    /// 跟随自动推荐。
    Auto,
    /// 用户手选档（仅双/三合法——单缓冲不在本条范围，档外即拒）。
    Force(u8),
}

/// 用户手选合法性（双/三之外一律非法，含 0/1/4 及以上）。
pub const fn override_valid(n: u8) -> bool {
    n == BUFFERS_DOUBLE || n == BUFFERS_TRIPLE
}

// ---------------------------------------------------------------------------
// 七、一次评估的结论
// ---------------------------------------------------------------------------

/// 策略器一次评估的完整结论（可逐字段对账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    /// 最终生效档（覆盖后）。
    pub buffers: u8,
    /// 自适应推荐档（覆盖前）。
    pub auto_buffers: u8,
    /// 主导理由。
    pub reason: Reason,
    /// VRR 仲裁裁决。
    pub vrr: VrrArbitration,
    /// 是否被用户手选覆盖。
    pub overridden: bool,
    /// 本轮是否出了失配建议。
    pub advised: bool,
}

// ---------------------------------------------------------------------------
// 八、策略器
// ---------------------------------------------------------------------------

/// 多缓冲策略自适应中枢（策略器）。
///
/// 现役缓冲数由呈现侧（F0046 链）经 [`PolicyAdvisor::report_active`]
/// 回填；评估是 O(1) 查表加常数规则；失配只出建议不改档。
#[derive(Clone, Copy, Debug)]
pub struct PolicyAdvisor {
    /// 现役缓冲数（呈现侧回填）。
    active: u8,
    /// 用户覆盖档。
    override_sel: Override,
    /// 连续失配计数。
    strikes: usize,
    /// 评估总次数。
    pub evaluations: u64,
    /// 失配建议总次数（每连续 [`MISMATCH_STRIKES`] 次计一次）。
    pub advices_total: u32,
    /// 策略张力总次数（延迟紧 × GPU 忙）。
    pub tension_total: u32,
    /// VRR 仲裁总次数（含挡立即与降档，不含视作定频）。
    pub arbitrations_total: u32,
    /// 窄窗视作定频总次数。
    pub vrr_fixed_total: u32,
    /// 覆盖被拒总次数。
    pub overrides_rejected: u32,
    /// 覆盖生效总次数。
    pub overrides_applied: u32,
    /// 手选与邮箱地板冲突的可见计数（不拦，但必须可见）。
    pub override_floor_conflicts: u32,
    /// 平滑过渡成功档数。
    pub transitions_applied: u32,
    /// 平滑过渡回退档数。
    pub transitions_rolled_back: u32,
    /// 最近一次自动推荐档（面板用；未评估过为 0）。
    last_auto: u8,
}

impl Default for PolicyAdvisor {
    fn default() -> Self {
        PolicyAdvisor::new()
    }
}

impl PolicyAdvisor {
    pub const fn new() -> PolicyAdvisor {
        PolicyAdvisor {
            active: 0,
            override_sel: Override::Auto,
            strikes: 0,
            evaluations: 0,
            advices_total: 0,
            tension_total: 0,
            arbitrations_total: 0,
            vrr_fixed_total: 0,
            overrides_rejected: 0,
            overrides_applied: 0,
            override_floor_conflicts: 0,
            transitions_applied: 0,
            transitions_rolled_back: 0,
            last_auto: 0,
        }
    }

    /// 呈现侧回填现役缓冲数（F0046 链的真实档位；现实就是现实，不校验
    /// 也不截断——链给 4 就按 4 记失配，这正是建议通道要抓的失配）。
    pub fn report_active(&mut self, n: u8) {
        self.active = n;
    }

    pub fn active(&self) -> u8 {
        self.active
    }

    pub fn last_auto(&self) -> u8 {
        self.last_auto
    }

    /// 设置用户覆盖。档外值：拒绝、计数、**保留原覆盖**（一次手误不该
    /// 把「已手选三缓冲」静默重置成自动）。
    pub fn set_override(&mut self, o: Override) -> Result<(), PolicyCode> {
        match o {
            Override::Auto => {
                self.override_sel = Override::Auto;
                Ok(())
            }
            Override::Force(n) => {
                if override_valid(n) {
                    self.override_sel = Override::Force(n);
                    Ok(())
                } else {
                    self.overrides_rejected = self.overrides_rejected.saturating_add(1);
                    Err(CODE_OVERRIDE_REJECTED)
                }
            }
        }
    }

    pub fn override_sel(&self) -> Override {
        self.override_sel
    }

    /// 评估一次（O(1)）。
    ///
    /// 规则次序刻意固定：**查表 → 丢帧升级 → 同步地板 → VRR 仲裁 → 用户
    /// 覆盖 → 失配建议**。同轮多规则命中时按此次序定主导理由；次序若
    /// 不确定，同一份遥测会给出不同结论，跨帧对账无从说起。
    pub fn evaluate(&mut self, tel: &Telemetry, latency_budget_us: u32) -> Result<Decision, PolicyCode> {
        tel.validate()?;
        self.evaluations = self.evaluations.saturating_add(1);

        let lat = tel.latency_class(latency_budget_us);
        let head = tel.headroom_class();

        let mut vrr = VrrArbitration::None;
        let mut vrr_fixed = false;

        // VRR 有效性：启用且跨度达标才参与联动，窄窗视作定频（裁决可见）。
        let vrr_effective = tel.vrr_enabled && tel.vrr_span_hz() >= VRR_NARROW_HZ;
        if tel.vrr_enabled && !vrr_effective {
            self.vrr_fixed_total = self.vrr_fixed_total.saturating_add(1);
            vrr_fixed = true;
        }

        // 1) 查表（表驱动）。
        let entry = tradeoff_entry(lat, head);
        let mut buffers = entry.buffers;
        let mut reason = entry.reason;

        // 张力计数（延迟紧 × GPU 忙：结论仍是双缓冲，但张力必须可见）。
        if lat.ordinal() == 0 && head.ordinal() == 0 {
            self.tension_total = self.tension_total.saturating_add(1);
        }

        // 2) 丢帧升级：仅双→三，绝不反向（丢帧不会因为缓冲多而变好）。
        if buffers == BUFFERS_DOUBLE && tel.dropped_recent >= DROPS_ESCALATE_AT {
            buffers = BUFFERS_TRIPLE;
            reason = Reason::DropEscalation;
        }

        // 3) 同步地板：邮箱类语义下界（与 F0046 的 mailbox=三缓冲对齐）。
        let floor = tel.sync.floor_buffers();
        if floor > buffers {
            buffers = floor;
            reason = Reason::SyncFloorMailbox;
        }

        // 4) VRR 仲裁：
        //    a. 生效 VRR × 立即类 ⇒ 冲突，仲裁保 VRR（立即被挡，档不变）；
        //    b. 生效 VRR × 纯查表三缓冲（非丢帧升级、非地板抬上来的、
        //       且当前无活跃丢帧信号）⇒ 降回双缓冲——VRR 已吸收帧间波动，
        //       吞吐档的第三块缓冲只剩排队延迟。丢帧场景与邮箱地板不被
        //       降档：前者是饥饿证据（查表虽是吞吐档，但丢帧达阈值时
        //       降回去就是重新丢帧），后者是语义下界（地板语义优先）。
        if vrr_effective && tel.sync.ordinal() == SyncDemand::ImmediateLike.ordinal() {
            self.arbitrations_total = self.arbitrations_total.saturating_add(1);
            vrr = VrrArbitration::BlockedImmediate;
        } else if vrr_effective
            && buffers == BUFFERS_TRIPLE
            && reason.ordinal() == Reason::TableThroughput.ordinal()
            && tel.dropped_recent < DROPS_ESCALATE_AT
        {
            buffers = BUFFERS_DOUBLE;
            reason = Reason::VrrDemotion;
            vrr = VrrArbitration::DemotedForVrr;
            self.arbitrations_total = self.arbitrations_total.saturating_add(1);
        }
        if vrr_fixed {
            vrr = VrrArbitration::TreatedFixed;
        }

        let auto_buffers = buffers;
        self.last_auto = auto_buffers;

        // 5) 用户覆盖：手选合法档即生效于自动推荐之上；与邮箱地板冲突
        //    不拦（用户主权）但冲突可见。
        let mut overridden = false;
        match self.override_sel {
            Override::Auto => {}
            Override::Force(n) => {
                // set_override 已保证 n 合法；此处仍是防御性闸（不为真）。
                if override_valid(n) {
                    if n != buffers {
                        overridden = true;
                        reason = Reason::UserOverride;
                        if floor > n {
                            self.override_floor_conflicts =
                                self.override_floor_conflicts.saturating_add(1);
                        }
                    }
                    buffers = n;
                    self.overrides_applied = self.overrides_applied.saturating_add(1);
                }
            }
        }

        // 6) 策略失配 → 建议（只建议，不改档）。连续 MISMATCH_STRIKES 次
        //    失配计一次建议并归零重计；一旦现役追上推荐立即归零。
        let mut advised = false;
        if self.active != buffers {
            self.strikes += 1;
            if self.strikes >= MISMATCH_STRIKES {
                self.advices_total = self.advices_total.saturating_add(1);
                self.strikes = 0;
                advised = true;
            }
        } else {
            self.strikes = 0;
        }

        Ok(Decision { buffers, auto_buffers, reason, vrr, overridden, advised })
    }

    /// 过渡结果回填（过渡器调用；applied 才改现役档）。
    pub fn record_transition(&mut self, applied: bool, to: u8) {
        if applied {
            self.transitions_applied = self.transitions_applied.saturating_add(1);
            self.active = to;
            self.strikes = 0;
        } else {
            self.transitions_rolled_back = self.transitions_rolled_back.saturating_add(1);
        }
    }

    /// 读屏面板（七行双语，只报聚合计数与策略事实）。
    pub fn a11y_lines(&self) -> [String; PANEL_LINES] {
        let override_line = match self.override_sel {
            Override::Auto => String::from("自动 / auto"),
            Override::Force(n) => format!("手选 {} 缓冲 / forced {} buffers", n, n),
        };
        [
            format!("现役缓冲数 / active buffers: {}", self.active),
            format!("自适应推荐 / auto recommendation: {}", self.last_auto),
            format!("用户覆盖 / user override: {}", override_line),
            format!(
                "VRR 定频视作 / vrr treated fixed: {}（仲裁 {}）",
                self.vrr_fixed_total, self.arbitrations_total
            ),
            format!(
                "失配建议 / mismatch advices: {}",
                self.advices_total
            ),
            format!(
                "平滑过渡 / transitions: 成功 {}，回退 {}",
                self.transitions_applied, self.transitions_rolled_back
            ),
            format!(
                "张力与覆盖冲突 / tension & override conflicts: {} 与 {}",
                self.tension_total, self.override_floor_conflicts
            ),
        ]
    }
}

// ---------------------------------------------------------------------------
// 九、过渡器（缓冲数变更的平滑过渡）
// ---------------------------------------------------------------------------

/// 过渡相。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionPhase {
    /// 评估中（起止档一致则直接完成）。
    Assess,
    /// 排空中（等飞帧引用归零）。
    Draining,
    /// 已应用（新档生效）。
    Applied,
    /// 已回退（旧档继续生效；失败留痕）。
    RolledBack,
}

impl TransitionPhase {
    pub const fn ordinal(self) -> usize {
        match self {
            TransitionPhase::Assess => 0,
            TransitionPhase::Draining => 1,
            TransitionPhase::Applied => 2,
            TransitionPhase::RolledBack => 3,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            TransitionPhase::Assess => "评估 / assess",
            TransitionPhase::Draining => "排空 / draining",
            TransitionPhase::Applied => "已应用 / applied",
            TransitionPhase::RolledBack => "已回退 / rolled back",
        }
    }

    pub const fn is_terminal(self) -> bool {
        self.ordinal() >= TransitionPhase::Applied.ordinal()
    }
}

/// 缓冲数变更的平滑过渡器。
///
/// **平滑的唯一定义**：新档只在静默点（无飞帧引用旧缓冲）应用。
/// 带着在飞的帧硬换缓冲数，等于让飞帧引用不存在的资源——这就是
/// 「切个缓冲数花屏了」的全部成因。排空卡死或应用段注入失败 ⇒
/// 回退旧档（锚点「切换失败→回退」），回退原因可查。
#[derive(Clone, Copy, Debug)]
pub struct BufferTransition {
    from: u8,
    to: u8,
    phase: TransitionPhase,
    /// 呈现侧上报的在飞帧引用数（旧缓冲）。
    inflight: u8,
    /// 已走步数（排空预算的计量单位）。
    steps: usize,
    /// 应用段演练故障注入。
    fault: bool,
    /// 回退原因码（未回退为 None）。
    rollback_code: Option<PolicyCode>,
}

impl BufferTransition {
    /// 立一份过渡计划。起止档都必须在双/三范围内——单缓冲不在本条
    /// 自适应范围，档外即拒（不静默夹到最近档）。
    pub fn plan(from: u8, to: u8) -> Result<BufferTransition, PolicyCode> {
        if !override_valid(from) || !override_valid(to) {
            return Err(CODE_BAD_BUFFER_COUNT);
        }
        Ok(BufferTransition {
            from,
            to,
            phase: TransitionPhase::Assess,
            inflight: 0,
            steps: 0,
            fault: false,
            rollback_code: None,
        })
    }

    pub fn from(&self) -> u8 {
        self.from
    }

    pub fn to(&self) -> u8 {
        self.to
    }

    pub fn phase(&self) -> TransitionPhase {
        self.phase
    }

    pub fn steps(&self) -> usize {
        self.steps
    }

    pub fn rollback_code(&self) -> Option<PolicyCode> {
        self.rollback_code
    }

    /// 呈现侧上报在飞帧引用数（每步前刷新）。
    pub fn report_inflight(&mut self, n: u8) {
        self.inflight = n;
    }

    /// 注入应用段故障（演练常态化：切换失败是常态不是异常）。
    pub fn inject_apply_fault(&mut self) {
        self.fault = true;
    }

    /// 推进一步（O(1)）。终相后调用恒返回终相。
    ///
    /// 应用只发生在 `step()` 内部且必须同时满足：相为 Draining、
    /// 在飞引用为零、无注入故障。外部不存在绕过静默点的应用路径。
    pub fn step(&mut self) -> TransitionPhase {
        if self.phase.is_terminal() {
            return self.phase;
        }
        self.steps += 1;
        match self.phase {
            TransitionPhase::Assess => {
                if self.from == self.to {
                    // 同档「切换」是 no-op：直接完成，不制造一次假排空。
                    self.phase = TransitionPhase::Applied;
                } else {
                    self.phase = TransitionPhase::Draining;
                }
            }
            TransitionPhase::Draining => {
                if self.steps > DRAIN_BUDGET_STEPS {
                    // 排空卡死 ⇒ 回退（切换失败→回退）。
                    self.phase = TransitionPhase::RolledBack;
                    self.rollback_code = Some(CODE_DRAIN_TIMEOUT);
                } else if self.inflight == 0 {
                    if self.fault {
                        self.phase = TransitionPhase::RolledBack;
                        self.rollback_code = Some(CODE_APPLY_FAULT);
                    } else {
                        self.phase = TransitionPhase::Applied;
                    }
                }
                // 在飞引用未清零 ⇒ 继续排空，绝不带飞帧应用。
            }
            // 终相不可达（开头已拦），match 需穷举故占位。
            TransitionPhase::Applied | TransitionPhase::RolledBack => {}
        }
        self.phase
    }

    /// 外部强行应用闸：非排空相或仍有飞帧引用时拒绝。
    ///
    /// 平滑过渡的守护面——调用方若想绕过 step() 直接应用，必须先过
    /// 这道静默点闸；带飞帧硬切在此被拒（[`CODE_APPLY_NOT_QUIESCENT`]）。
    pub fn force_apply(&mut self) -> Result<(), PolicyCode> {
        if self.phase.ordinal() != TransitionPhase::Draining.ordinal() {
            return Err(CODE_APPLY_NOT_QUIESCENT);
        }
        if self.inflight != 0 {
            return Err(CODE_APPLY_NOT_QUIESCENT);
        }
        if self.fault {
            self.phase = TransitionPhase::RolledBack;
            self.rollback_code = Some(CODE_APPLY_FAULT);
            return Err(CODE_APPLY_FAULT);
        }
        self.phase = TransitionPhase::Applied;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 十、判据
// ---------------------------------------------------------------------------

pub fn run_vea47_checks() -> CheckSet {
    let mut s = CheckSet::new("vea47_multibuf");

    // --- 判据 1：取舍表驱动——四格穷举与判据侧独立重算逐格对账 ---
    {
        // 独立重算期望表（不读 TRADEOFF_TABLE，逐格字面写死）。
        // 轴序与循环一致：j=0 ⇒ Busy，j=1 ⇒ Idle。
        let expect: [[u8; 2]; 2] = [
            [BUFFERS_DOUBLE, BUFFERS_DOUBLE], // [Tight][Busy] [Tight][Idle]
            [BUFFERS_TRIPLE, BUFFERS_DOUBLE], // [Loose][Busy] [Loose][Idle]
        ];
        let mut ok = true;
        let mut i = 0usize;
        while i < AXIS_KINDS {
            let mut j = 0usize;
            while j < AXIS_KINDS {
                let lat = if i == 0 { LatencyClass::Tight } else { LatencyClass::Loose };
                let head = if j == 0 { Headroom::Busy } else { Headroom::Idle };
                let e = tradeoff_entry(lat, head);
                if e.buffers != expect[i][j] {
                    ok = false;
                }
                if !override_valid(e.buffers) {
                    ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        // 唯一默认三缓冲格是 Loose×Busy（查表直证，防期望表自身抄错）。
        let loose_busy = tradeoff_entry(LatencyClass::Loose, Headroom::Busy);
        let tight_idle = tradeoff_entry(LatencyClass::Tight, Headroom::Idle);
        s.add(
            "A47-取舍表-四格穷举且唯一默认三缓冲格",
            ok
                && loose_busy.buffers == BUFFERS_TRIPLE
                && loose_busy.reason.ordinal() == Reason::TableThroughput.ordinal()
                && tight_idle.buffers == BUFFERS_DOUBLE
                && tight_idle.reason.ordinal() == Reason::TableLatencyIdle.ordinal(),
            "2×2 取舍表逐格与独立重算一致；仅 Loose×Busy 默认三缓冲（理由=吞吐），Tight×Idle 双缓冲（理由=延迟）",
        );
    }

    // --- 判据 2：自适应选择——分桶边界不 off-by-one ---
    {
        // 延迟档：恰好等于预算即 Tight（预算是上界）。
        let tel_edge = Telemetry::new(5000, 1000, 16667, 10, 0, false, 0, 0, SyncDemand::FifoLike);
        let tight = tel_edge.latency_class(5000).ordinal() == LatencyClass::Tight.ordinal();
        let loose = tel_edge.latency_class(4999).ordinal() == LatencyClass::Loose.ordinal();
        // 余量档：占用恰好 85 即 Busy；帧耗时≥90% 周期（15001/16667）即
        // Busy；贴线之下（15000，≈89.998%）即 Idle——边界不 off-by-one。
        let busy_pct = Telemetry::new(1, 1000, 16667, GPU_BUSY_PCT, 0, false, 0, 0, SyncDemand::FifoLike)
            .headroom_class()
            .ordinal()
            == Headroom::Busy.ordinal();
        let busy_ratio = Telemetry::new(1, 15001, 16667, 0, 0, false, 0, 0, SyncDemand::FifoLike)
            .headroom_class()
            .ordinal()
            == Headroom::Busy.ordinal();
        let idle_ratio = Telemetry::new(1, 15000, 16667, 0, 0, false, 0, 0, SyncDemand::FifoLike)
            .headroom_class()
            .ordinal()
            == Headroom::Idle.ordinal();
        s.add(
            "A47-自适应-分桶边界恰好贴线即紧/忙",
            tight && loose && busy_pct && busy_ratio && idle_ratio && BUSY_RATIO_NUM == 9,
            "P95==预算 ⇒ Tight（>预算才 Loose）；占用==85 ⇒ Busy；帧耗时 15001/16667（≥90%）⇒ Busy，15000（≈89.998%）⇒ Idle",
        );
    }

    // --- 判据 3：丢帧升级——达到阈值升一档、未达不升、三缓冲不反降 ---
    {
        let mut base = PolicyAdvisor::new();
        base.report_active(BUFFERS_DOUBLE);
        let no_drop = Telemetry::new(1, 1000, 16667, 0, DROPS_ESCALATE_AT - 1, false, 0, 0, SyncDemand::FifoLike);
        let d0 = base.evaluate(&no_drop, 1_000_000);
        let mut esc = PolicyAdvisor::new();
        esc.report_active(BUFFERS_DOUBLE);
        let drop = Telemetry::new(1, 1000, 16667, 0, DROPS_ESCALATE_AT, false, 0, 0, SyncDemand::FifoLike);
        let d1 = esc.evaluate(&drop, 1_000_000);
        // 三缓冲在丢帧下不被反降（丢帧只升不降）：用 Loose×Busy 底格验证。
        let mut tri = PolicyAdvisor::new();
        tri.report_active(BUFFERS_TRIPLE);
        let busy_drop = Telemetry::new(2_000_000, 20000, 16667, 10, 9, false, 0, 0, SyncDemand::FifoLike);
        let d2 = tri.evaluate(&busy_drop, 1_000_000);
        s.add(
            "A47-自适应-丢帧升级单双向正确",
            matches!(d0, Ok(dec) if dec.buffers == BUFFERS_DOUBLE)
                && matches!(d1, Ok(dec) if dec.buffers == BUFFERS_TRIPLE && dec.reason.ordinal() == Reason::DropEscalation.ordinal())
                && matches!(d2, Ok(dec) if dec.buffers == BUFFERS_TRIPLE && dec.reason.ordinal() == Reason::TableThroughput.ordinal()),
            "丢帧未达阈值保持双缓冲；达阈值升三缓冲且理由=丢帧升级；已三缓冲时不被丢帧信号改动（只升不降）",
        );
    }

    // --- 判据 4：同步地板联动（A48 接缝）——邮箱类抬三缓冲 ---
    {
        let mut a = PolicyAdvisor::new();
        a.report_active(BUFFERS_DOUBLE);
        // 延迟紧 × GPU 闲 × 邮箱类：查表双缓冲，地板抬到三。
        let tel = Telemetry::new(1, 1000, 16667, 0, 0, false, 0, 0, SyncDemand::MailboxLike);
        let d = a.evaluate(&tel, 1_000_000);
        // 保序/立即类无地板：同遥测保持双缓冲。
        let mut b = PolicyAdvisor::new();
        b.report_active(BUFFERS_DOUBLE);
        let tel_f = Telemetry::new(1, 1000, 16667, 0, 0, false, 0, 0, SyncDemand::FifoLike);
        let df = b.evaluate(&tel_f, 1_000_000);
        s.add(
            "A47-联动-邮箱地板抬三缓冲且保序无地板",
            matches!(d, Ok(dec) if dec.buffers == BUFFERS_TRIPLE && dec.reason.ordinal() == Reason::SyncFloorMailbox.ordinal())
                && matches!(df, Ok(dec) if dec.buffers == BUFFERS_DOUBLE)
                && SYNC_MAILBOX_MIN_BUFFERS == BUFFERS_TRIPLE
                && SyncDemand::MailboxLike.floor_buffers() == BUFFERS_TRIPLE
                && SyncDemand::FifoLike.floor_buffers() == 0
                && SyncDemand::ImmediateLike.floor_buffers() == 0,
            "邮箱类同步把推荐抬到三缓冲（理由=邮箱地板），与 F0046 mailbox=三缓冲语义对齐；保序/立即类不设地板",
        );
    }

    // --- 判据 5：VRR 联动与仲裁——窄窗定频、冲突挡立即、降档与免疫 ---
    {
        // a. 窄窗（跨度 5 < 10）：视作定频，不降档不仲裁。
        //    遥测为 Tight×Busy ⇒ 查表双缓冲；窄窗不产生任何仲裁计数。
        let mut a = PolicyAdvisor::new();
        a.report_active(BUFFERS_DOUBLE);
        let narrow = Telemetry::new(1, 20000, 16667, 10, 0, true, 60, 65, SyncDemand::FifoLike);
        let da = a.evaluate(&narrow, 1_000_000);
        // b. 生效 VRR × 立即类：仲裁挡立即，档不变（Tight×Busy ⇒ 双缓冲）。
        let mut b = PolicyAdvisor::new();
        b.report_active(BUFFERS_DOUBLE);
        let immediate = Telemetry::new(1, 20000, 16667, 10, 0, true, 48, 144, SyncDemand::ImmediateLike);
        let db = b.evaluate(&immediate, 1_000_000);
        // c. 生效 VRR × 纯查表三缓冲（Loose×Busy）：降回双缓冲。
        let mut c = PolicyAdvisor::new();
        c.report_active(BUFFERS_DOUBLE);
        let throughput_vrr = Telemetry::new(2_000_000, 20000, 16667, 10, 0, true, 48, 144, SyncDemand::FifoLike);
        let dc = c.evaluate(&throughput_vrr, 1_000_000);
        // d. 生效 VRR × 邮箱地板抬的三缓冲：地板语义优先，不被降档。
        let mut d = PolicyAdvisor::new();
        d.report_active(BUFFERS_DOUBLE);
        let floor_vrr = Telemetry::new(1, 1000, 16667, 0, 0, true, 48, 144, SyncDemand::MailboxLike);
        let dd = d.evaluate(&floor_vrr, 1_000_000);
        // e. 生效 VRR × 已三缓冲（查表吞吐档）+ 丢帧：已是最高档，丢帧
        //    信号无操作，降档不得推翻（降回去就是重新丢帧）。
        let mut e = PolicyAdvisor::new();
        e.report_active(BUFFERS_DOUBLE);
        let drop_vrr = Telemetry::new(2_000_000, 20000, 16667, 10, DROPS_ESCALATE_AT, true, 48, 144, SyncDemand::FifoLike);
        let de = e.evaluate(&drop_vrr, 1_000_000);
        s.add(
            "A47-VRR-窄窗定频且挡立即且降档有免疫",
            matches!(da, Ok(dec) if dec.buffers == BUFFERS_DOUBLE && dec.vrr.ordinal() == VrrArbitration::TreatedFixed.ordinal())
                && a.vrr_fixed_total == 1
                && a.arbitrations_total == 0
                && matches!(db, Ok(dec) if dec.vrr.ordinal() == VrrArbitration::BlockedImmediate.ordinal() && dec.buffers == BUFFERS_DOUBLE)
                && b.arbitrations_total == 1
                && matches!(dc, Ok(dec) if dec.buffers == BUFFERS_DOUBLE && dec.vrr.ordinal() == VrrArbitration::DemotedForVrr.ordinal())
                && c.arbitrations_total == 1
                && matches!(dd, Ok(dec) if dec.buffers == BUFFERS_TRIPLE && dec.reason.ordinal() == Reason::SyncFloorMailbox.ordinal())
                && matches!(de, Ok(dec) if dec.buffers == BUFFERS_TRIPLE && dec.reason.ordinal() == Reason::TableThroughput.ordinal()),
            "窄窗 VRR 视作定频（fixed 计数 1、仲裁 0）；生效 VRR×立即类被仲裁挡（档不变）；纯查表三缓冲被降回双（裁决=因VRR降档）；邮箱地板抬升与已三缓冲的丢帧场景对降档免疫（语义下界与饥饿证据优先）",
        );
    }

    // --- 判据 6：用户覆盖——合法手选生效、档外拒绝且保留原覆盖 ---
    {
        let mut a = PolicyAdvisor::new();
        a.report_active(BUFFERS_DOUBLE);
        // 档外手选：拒绝 + 计数 + 原覆盖（Auto）保留。
        let r1 = a.set_override(Override::Force(4));
        let r2 = a.set_override(Override::Force(0));
        // 合法手选：生效于自动推荐之上。
        let r3 = a.set_override(Override::Force(BUFFERS_TRIPLE));
        let tel = Telemetry::new(1, 1000, 16667, 0, 0, false, 0, 0, SyncDemand::FifoLike);
        let d = a.evaluate(&tel, 1_000_000);
        s.add(
            "A47-覆盖-档外拒绝保留原档且合法手选生效",
            r1 == Err(CODE_OVERRIDE_REJECTED)
                && r2 == Err(CODE_OVERRIDE_REJECTED)
                && r3.is_ok()
                && a.overrides_rejected == 2
                && matches!(a.override_sel(), Override::Force(BUFFERS_TRIPLE))
                && matches!(d, Ok(dec) if dec.buffers == BUFFERS_TRIPLE && dec.auto_buffers == BUFFERS_DOUBLE && dec.overridden && dec.reason.ordinal() == Reason::UserOverride.ordinal())
                && a.overrides_applied >= 1,
            "Force(4)/Force(0) 被拒且计数、原覆盖不被重置；Force(3) 生效：生效档 3、自动推荐仍 2、overridden 与理由=手选",
        );
    }

    // --- 判据 7：策略失配→建议——恰在阈值出一次、追上即归零 ---
    {
        let mut a = PolicyAdvisor::new();
        a.report_active(BUFFERS_DOUBLE); // 现役双缓冲
        let tel = Telemetry::new(2_000_000, 20000, 16667, 0, 0, false, 0, 0, SyncDemand::FifoLike); // Loose×Busy ⇒ 推荐三
        let mut fired = 0u32;
        let mut i = 0usize;
        while i < MISMATCH_STRIKES + 2 {
            if let Ok(dec) = a.evaluate(&tel, 1_000_000) {
                if dec.advised {
                    fired += 1;
                }
            }
            i += 1;
        }
        // 前strikes 次:第4次出建议归零;第5、6次重新计1、2次,不再出。
        // 现役追上推荐后立即归零。
        a.report_active(BUFFERS_TRIPLE);
        let d_ok = a.evaluate(&tel, 1_000_000);
        s.add(
            "A47-建议-恰在阈值出一次且追上即归零",
            fired == 1
                && a.advices_total == 1
                && matches!(d_ok, Ok(dec) if dec.buffers == BUFFERS_TRIPLE && !dec.advised)
                && a.advices_total == 1,
            "连续失配 4 次恰在第 4 次出一次建议并归零重计（第 5、6 次不再出）；现役追上推荐立即归零且不再出建议",
        );
    }

    // --- 判据 8：平滑过渡——排空到零才应用、同档 no-op、终相恒定 ---
    {
        let mut t = match BufferTransition::plan(BUFFERS_DOUBLE, BUFFERS_TRIPLE) {
            Ok(x) => x,
            Err(_) => {
                s.fail("A47-过渡-计划构造失败", "双→三计划必须合法");
                return s;
            }
        };
        // 第一步：Assess → Draining。
        let p1 = t.step();
        // 有飞帧时不应用。
        t.report_inflight(2);
        let p2 = t.step();
        // 归零后才应用。
        t.report_inflight(0);
        let p3 = t.step();
        // 终相后 step 恒返回终相。
        let p4 = t.step();
        // 同档 no-op：一步直达 Applied。
        let noop = match BufferTransition::plan(BUFFERS_DOUBLE, BUFFERS_DOUBLE) {
            Ok(mut x) => {
                let a = x.step();
                a == TransitionPhase::Applied && x.phase() == TransitionPhase::Applied
            }
            Err(_) => false,
        };
        s.add(
            "A47-过渡-排空到零才应用且同档no-op",
            p1 == TransitionPhase::Draining
                && p2 == TransitionPhase::Draining
                && p3 == TransitionPhase::Applied
                && p4 == TransitionPhase::Applied
                && noop
                && t.from() == BUFFERS_DOUBLE
                && t.to() == BUFFERS_TRIPLE,
            "Assess→Draining；飞帧为 2 时不应用（仍 Draining）；归零后应用（Applied）；终相 step 恒定；同档一步直达 Applied 不制造假排空",
        );
    }

    // --- 判据 9：切换失败→回退——注入故障回退且旧档不变 ---
    {
        let mut t = match BufferTransition::plan(BUFFERS_DOUBLE, BUFFERS_TRIPLE) {
            Ok(x) => x,
            Err(_) => {
                s.fail("A47-回退-计划构造失败", "双→三计划必须合法");
                return s;
            }
        };
        t.inject_apply_fault();
        let _ = t.step(); // Assess → Draining
        t.report_inflight(0);
        let p = t.step(); // 排空到零，但注入故障 ⇒ 回退
        let mut a = PolicyAdvisor::new();
        a.report_active(BUFFERS_DOUBLE);
        a.record_transition(false, BUFFERS_TRIPLE);
        s.add(
            "A47-回退-注入故障回退且现役档不变",
            p == TransitionPhase::RolledBack
                && t.rollback_code() == Some(CODE_APPLY_FAULT)
                && a.active() == BUFFERS_DOUBLE
                && a.transitions_rolled_back == 1
                && a.transitions_applied == 0,
            "应用段注入故障 ⇒ RolledBack 且回退原因=APPLY_FAULT；策略器回填后现役档仍是旧档（切换失败→回退）",
        );
    }

    // --- 判据 10：排空超预算→回退 + 非静默点强应用被拒 ---
    {
        // 排空卡死：在飞引用恒大于零。
        let mut t = match BufferTransition::plan(BUFFERS_DOUBLE, BUFFERS_TRIPLE) {
            Ok(x) => x,
            Err(_) => {
                s.fail("A47-超时-计划构造失败", "双→三计划必须合法");
                return s;
            }
        };
        let _ = t.step(); // Assess → Draining
        t.report_inflight(1);
        let mut rolled = false;
        let mut n = 0usize;
        loop {
            if n > DRAIN_BUDGET_STEPS + 4 {
                break;
            }
            if t.step() == TransitionPhase::RolledBack {
                rolled = true;
                break;
            }
            n += 1;
        }
        // 非静默点强应用：Assess 相直接强应用被拒。
        let mut t2 = match BufferTransition::plan(BUFFERS_DOUBLE, BUFFERS_TRIPLE) {
            Ok(x) => x,
            Err(_) => {
                s.fail("A47-强应用-计划构造失败", "双→三计划必须合法");
                return s;
            }
        };
        let reject_phase = t2.force_apply() == Err(CODE_APPLY_NOT_QUIESCENT);
        // Draining 但有飞帧：同样被拒。
        let _ = t2.step(); // → Draining
        t2.report_inflight(3);
        let reject_inflight = t2.force_apply() == Err(CODE_APPLY_NOT_QUIESCENT);
        s.add(
            "A47-超时-排空卡死回退且强应用被静默点闸拒绝",
            rolled
                && t.rollback_code() == Some(CODE_DRAIN_TIMEOUT)
                && reject_phase
                && reject_inflight,
            "在飞引用恒大于零时排空超预算 ⇒ RolledBack（原因=DRAIN_TIMEOUT）；Assess 相与带飞帧的 Draining 相强应用均被 CODE_APPLY_NOT_QUIESCENT 拒",
        );
    }

    // --- 判据 11：过渡计划档位防护——单缓冲与档外值即拒 ---
    {
        let bad1 = matches!(BufferTransition::plan(1, BUFFERS_TRIPLE), Err(CODE_BAD_BUFFER_COUNT));
        let bad2 = matches!(BufferTransition::plan(BUFFERS_DOUBLE, 0), Err(CODE_BAD_BUFFER_COUNT));
        let bad3 = matches!(BufferTransition::plan(BUFFERS_DOUBLE, 4), Err(CODE_BAD_BUFFER_COUNT));
        s.add(
            "A47-防护-过渡计划档外值即拒不静默夹档",
            bad1 && bad2 && bad3 && override_valid(BUFFERS_DOUBLE) && override_valid(BUFFERS_TRIPLE) && !override_valid(1) && !override_valid(4),
            "起止档不在双/三范围（单缓冲、零、四）一律 CODE_BAD_BUFFER_COUNT，不静默夹到最近档",
        );
    }

    // --- 判据 12：遥测防护——零值/越界/VRR 非法范围即拒 ---
    {
        let zero_ft = Telemetry::new(1, 0, 16667, 0, 0, false, 0, 0, SyncDemand::FifoLike).validate();
        let zero_rp = Telemetry::new(1, 1000, 0, 0, 0, false, 0, 0, SyncDemand::FifoLike).validate();
        let busy_over = Telemetry::new(1, 1000, 16667, 101, 0, false, 0, 0, SyncDemand::FifoLike).validate();
        let vrr_bad1 = Telemetry::new(1, 1000, 16667, 0, 0, true, 0, 144, SyncDemand::FifoLike).validate();
        let vrr_bad2 = Telemetry::new(1, 1000, 16667, 0, 0, true, 200, 144, SyncDemand::FifoLike).validate();
        let good = Telemetry::new(1, 1000, 16667, 100, 0, true, 48, 144, SyncDemand::FifoLike).validate();
        s.add(
            "A47-防护-遥测零值越界与VRR非法范围即拒",
            zero_ft == Err(CODE_BAD_TELEMETRY)
                && zero_rp == Err(CODE_BAD_TELEMETRY)
                && busy_over == Err(CODE_BAD_TELEMETRY)
                && vrr_bad1 == Err(CODE_VRR_BAD_RANGE)
                && vrr_bad2 == Err(CODE_VRR_BAD_RANGE)
                && good.is_ok(),
            "零帧耗时/零刷新周期/GPU 占用 101 ⇒ BAD_TELEMETRY；VRR min=0 或 min>max ⇒ VRR_BAD_RANGE；全边界值（占用恰 100、min==max）合法",
        );
    }

    // --- 判据 13：出货接缝——决策 → 过渡 → 现役档真被更新 ---
    //
    // **关键门禁**：判据 3~10 各测各的组件。若策略器与过渡器之间没有
    // 真接缝，两者可以各自全绿而现役档从未真的变过。故此处走完整链：
    // 评估出推荐 → 立过渡计划 → 排空 → 应用 → 回填现役档。
    {
        let mut a = PolicyAdvisor::new();
        a.report_active(BUFFERS_DOUBLE);
        // Loose×Busy ⇒ 推荐三缓冲。
        let tel = Telemetry::new(2_000_000, 20000, 16667, 10, 0, false, 0, 0, SyncDemand::FifoLike);
        let dec = match a.evaluate(&tel, 1_000_000) {
            Ok(d) => d,
            Err(_) => {
                s.fail("A47-接缝-评估失败", "合法遥测必须通过");
                return s;
            }
        };
        let mut t = match BufferTransition::plan(a.active(), dec.buffers) {
            Ok(x) => x,
            Err(_) => {
                s.fail("A47-接缝-计划失败", "双→三计划必须合法");
                return s;
            }
        };
        let mut applied = TransitionPhase::Assess;
        let mut guard = 0usize;
        while !applied.is_terminal() && guard <= DRAIN_BUDGET_STEPS + 4 {
            t.report_inflight(0);
            applied = t.step();
            guard += 1;
        }
        let done = applied == TransitionPhase::Applied;
        a.record_transition(done, dec.buffers);
        s.add(
            "A47-接缝-决策经过渡真更新现役档",
            done
                && dec.buffers == BUFFERS_TRIPLE
                && a.active() == BUFFERS_TRIPLE
                && a.transitions_applied == 1
                && a.strikes == 0,
            "评估推荐三缓冲 → 过渡计划(双→三) → 排空应用 → 回填后现役档=3、应用计数 1、失配计数清零（策略器与过渡器在出货路径上串联）",
        );
    }

    // --- 判据 14：O(1) 与零 panic 面——枚举线编码往返与诊断码段 ---
    {
        let mut ok = true;
        let mut i = 0usize;
        while i < 3 {
            let sd = SyncDemand::ALL[i];
            if sd.ordinal() != i {
                ok = false;
            }
            match SyncDemand::from_wire(sd.wire()) {
                Some(b) => {
                    if b.ordinal() != sd.ordinal() {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            if sd.label().is_empty() {
                ok = false;
            }
            i += 1;
        }
        // 越界线编码返 None（零 panic 面）。
        ok = ok && SyncDemand::from_wire(200).is_none();
        // 诊断码：段内、互异、非零段前缀。
        let codes = [
            CODE_BAD_TELEMETRY,
            CODE_OVERRIDE_REJECTED,
            CODE_VRR_BAD_RANGE,
            CODE_VRR_CONFLICT,
            CODE_DRAIN_TIMEOUT,
            CODE_APPLY_FAULT,
            CODE_APPLY_NOT_QUIESCENT,
            CODE_PANEL_SHAPE,
            CODE_BAD_BUFFER_COUNT,
        ];
        let mut distinct = true;
        let mut i = 0usize;
        while i < codes.len() {
            if (codes[i] & 0xFF00) != 0x4700 {
                ok = false;
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
                j += 1;
            }
            i += 1;
        }
        // 理由码标签全部非空（读屏可达的前提）。
        let reasons = [
            Reason::TableLatencyIdle,
            Reason::TableTension,
            Reason::TableIdle,
            Reason::TableThroughput,
            Reason::DropEscalation,
            Reason::SyncFloorMailbox,
            Reason::VrrDemotion,
            Reason::UserOverride,
        ];
        let mut labels_ok = true;
        let mut i = 0usize;
        while i < Reason::ALL_LEN {
            if reasons[i].label().is_empty() {
                labels_ok = false;
            }
            i += 1;
        }
        s.add(
            "A47-零panic-线编码往返与诊断码段独占",
            ok && distinct && labels_ok && Reason::ALL_LEN == 8 && VrrArbitration::ALL_LEN == 4,
            "SyncDemand 线编码往返自洽、越界返 None；9 个诊断码全部落在独占 0x47xx 段且互异；8 个理由码与 4 个仲裁裁决标签全部非空",
        );
    }

    // --- 判据 15：面板七行双语且逐行绑定聚合量 ---
    {
        let mut a = PolicyAdvisor::new();
        a.report_active(BUFFERS_DOUBLE);
        let _ = a.set_override(Override::Force(BUFFERS_TRIPLE));
        let tel = Telemetry::new(2_000_000, 20000, 16667, 10, 0, false, 0, 0, SyncDemand::FifoLike);
        let mut i = 0usize;
        while i < MISMATCH_STRIKES {
            let _ = a.evaluate(&tel, 1_000_000);
            i += 1;
        }
        let lines = a.a11y_lines();
        let all_nonempty = lines.iter().all(|l| !l.is_empty());
        s.add(
            "A47-面板-七行双语且逐行绑定聚合量",
            lines.len() == PANEL_LINES
                && all_nonempty
                && lines[0].contains("active buffers: 2")
                && lines[1].contains("auto recommendation: 3")
                && lines[2].contains("forced 3 buffers")
                && lines[3].contains("vrr treated fixed: 0")
                && lines[4].contains("mismatch advices: 1")
                && lines[6].contains("tension")
                // 双语：每行含 ASCII 键名
                && lines.iter().all(|l| l.chars().any(|c| c.is_ascii_alphabetic())),
            "面板七行逐行绑定：现役 2 / 推荐 3 / 手选 3 / VRR 定频 0 / 失配建议恰 1（连续失配 4 次）/ 过渡与张力行非空，每行双语",
        );
    }

    s
}
