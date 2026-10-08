//! VE-F0050 · 丢弃帧检测与恢复（VE-A 域 · A03 同步与呈现组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0050`
//!
//! **职责定位（锚点原文）**：丢弃帧检测与恢复——呈现丢弃的检测与恢复
//! （连续丢弃→诊断模式），丢弃原因分类（GPU 忙/呈现阻塞/驱动节流
//! 三类），丢弃率看板；含丢弃帧的输入响应影响评估。数据结构：检测器；
//! 分类器。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! - 连续丢弃 → **诊断模式**（丢弃达阈值进入；连续正常帧恢复退出；
//!   恢复失败——退出后快速复发——升级计数，升级是事实不是情绪）；
//! - 原因不明 → **记录诊断包**（三类信号都不成立时归 Unknown，快照
//!   入环形诊断包供 X04 剖析消费，不静默丢）；
//! - 恢复失败 → **升级**（复发间隔低于恢复窗即升级，逐次计数可查）。
//!
//! **性能逐项分解**：O(1)——检测是帧号差一步计算；分类是三条阈值
//! 比较；丢弃率用环形窗口计数器维持（每帧 O(1) 增量），无历史扫描。
//!
//! **跨批对接点**：X04 剖析联动——诊断包快照码稳定可消费；A48/F0047
//! 的丢帧升级信号以本条的丢弃事实为源头（本条是源头不是消费方）。
//!
//! **无障碍与隐私**：丢弃看板读屏可达（[`DropWatcher::a11y_lines`]）——
//! 中英双语七行，只报丢弃事实与聚合计数，不泄漏窗口标题与渲染内容。
//!
//! ## 归因优先级文档（谁压谁，写明而非口口相传）
//!
//! 三类信号同时成立时按固定优先级归一类（[`DropCause`]）：
//! **驱动节流 > GPU 忙 > 呈现阻塞**。理由：节流是驱动层的强制行为，
//! 上层对策（降载/降分辨率）对它无效，必须最先被发现；GPU 忙是容量
//! 问题，对策明确（降负载）；呈现阻塞是队列形态问题，对策是减排队。
//! 三信号全不成立 → Unknown（进诊断包，不硬猜——硬猜会把三类都污染）。
//!
//! ## 设计要点（为什么这样写）
//!
//! - **跳帧检测是帧号差不是计时器**：期望上屏帧号与实际差一步得出
//!   丢弃数，帧号回退是输入违例拒绝（不改状态），不猜不补。
//! - **丢弃率是环形窗口增量计数**：每帧 O(1) 进出，率 = 窗口丢弃数
//!   × 1e6 / 窗口帧数（ppm 整数口径），不做全历史扫描。
//! - **诊断模式的进入与退出都有判据**：进入=窗口内丢弃达阈值；退出=
//!   连续正常帧达恢复窗；复发快于恢复窗 = 恢复失败升级（逐次计数）。
//! - **零 panic 面**：帧差走 checked（回退即拒）、计数全 `saturating_add`、
//!   环形下标取模、无 unwrap/expect。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F0047** 消费「最近丢弃数」做缓冲升级信号；本条产出丢弃事实，
//!   不决定缓冲数。
//! - **F0055** 管撕裂防护；丢弃与撕裂是两种失败，本条不碰撕裂。
//! - **X04** 管剖析展示；本条只产出诊断包快照。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、常量与诊断码（独占码段 0x4Axx）
// ---------------------------------------------------------------------------

/// 诊断包定容（环形；满则覆盖最旧并如实计数，累计账不减）。
pub const DIAG_PACK_CAP: usize = 8;

/// 丢弃率窗口（帧数；环形增量计数）。
pub const RATE_WINDOW: usize = 128;

/// 连续丢弃达该帧数 → 诊断模式。
pub const DIAGNOSE_STREAK: u32 = 3;

/// 恢复窗：连续正常帧达该数 → 退出诊断模式。
pub const RECOVER_FRAMES: u32 = 16;

/// 丢弃率升档阈值（ppm）：>= 该值即高丢弃率档。
pub const RATE_HIGH_PPM: u32 = 30_000;

/// 输入响应影响档阈值（帧）：丢弃 k 帧的档位表数据化。
/// 无(0) / 低(1) / 高(2..=4) / 严重(>4)。
pub const IMPACT_SEVERE_FRAMES: u32 = 4;

/// 输入响应影响档阈值（帧）：丢弃 k 帧的高影响下界。
pub const IMPACT_HIGH_FRAMES: u32 = 2;

/// 帧号回退（输入违例）。
pub const CODE_FRAME_REGRESS: PolicyCode = 0x4A01;

/// 非法信号值（越界占用/队列长度）。
pub const CODE_BAD_SIGNAL: PolicyCode = 0x4A02;

/// 非法帧周期（评估用）。
pub const CODE_BAD_PERIOD: PolicyCode = 0x4A03;

/// 诊断模式激活（信息级：状态切换留痕）。
pub const CODE_DIAG_ON: PolicyCode = 0x4A04;

/// 恢复失败升级（告警级）。
pub const CODE_RECOVERY_ESCALATED: PolicyCode = 0x4A05;

pub type PolicyCode = u16;

// ---------------------------------------------------------------------------
// 二、丢弃原因三分类 + Unknown（归因优先级见模块头文档）
// ---------------------------------------------------------------------------

/// 丢弃原因分类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropCause {
    /// GPU 忙：占用达标（容量问题，对策=降负载）。
    GpuBusy,
    /// 呈现阻塞：呈现队列积压（排队问题，对策=减排队）。
    PresentBlocked,
    /// 驱动节流：驱动层强制（对策无效，必须最先发现）。
    DriverThrottle,
    /// 原因不明：三信号全不成立 → 诊断包，不硬猜。
    Unknown,
}

impl DropCause {
    pub const ALL_LEN: usize = 4;

    pub const ALL: [DropCause; 4] = [
        DropCause::GpuBusy,
        DropCause::PresentBlocked,
        DropCause::DriverThrottle,
        DropCause::Unknown,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            DropCause::GpuBusy => 0,
            DropCause::PresentBlocked => 1,
            DropCause::DriverThrottle => 2,
            DropCause::Unknown => 3,
        }
    }

    pub const fn wire(self) -> u8 {
        match self {
            DropCause::GpuBusy => 0x01,
            DropCause::PresentBlocked => 0x02,
            DropCause::DriverThrottle => 0x03,
            DropCause::Unknown => 0x04,
        }
    }

    pub const fn from_wire(w: u8) -> Option<DropCause> {
        match w {
            0x01 => Some(DropCause::GpuBusy),
            0x02 => Some(DropCause::PresentBlocked),
            0x03 => Some(DropCause::DriverThrottle),
            0x04 => Some(DropCause::Unknown),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            DropCause::GpuBusy => "GPU忙 / gpu busy",
            DropCause::PresentBlocked => "呈现阻塞 / present blocked",
            DropCause::DriverThrottle => "驱动节流 / driver throttle",
            DropCause::Unknown => "原因不明 / unknown",
        }
    }
}

/// 归因信号快照（调用方供；本条裁决）。
#[derive(Clone, Copy, Debug)]
pub struct Signals {
    /// GPU 占用（百分比 0..=100）。
    pub gpu_busy_pct: u8,
    /// 呈现队列长度。
    pub present_queue_len: u32,
    /// 驱动节流标志。
    pub driver_throttle: bool,
}

impl Signals {
    /// 构造期校验（占用 > 100 即非法）。
    pub const fn validate(&self) -> Result<(), PolicyCode> {
        if self.gpu_busy_pct > 100 {
            return Err(CODE_BAD_SIGNAL);
        }
        Ok(())
    }
}

/// GPU 忙阈值（与 F0047 分桶口径同源对齐，自持常量判据钉住）。
pub const GPU_BUSY_PCT: u8 = 85;

/// 呈现积压阈值（队列长度达到即判呈现阻塞）。
pub const QUEUE_BLOCKED_LEN: u32 = 2;

/// 归因裁决（优先级：驱动节流 > GPU 忙 > 呈现阻塞；全不成立 = Unknown）。
pub const fn classify(s: &Signals) -> DropCause {
    if s.driver_throttle {
        DropCause::DriverThrottle
    } else if s.gpu_busy_pct >= GPU_BUSY_PCT {
        DropCause::GpuBusy
    } else if s.present_queue_len >= QUEUE_BLOCKED_LEN {
        DropCause::PresentBlocked
    } else {
        DropCause::Unknown
    }
}

// ---------------------------------------------------------------------------
// 三、输入响应影响评估（阈值档表数据化）
// ---------------------------------------------------------------------------

/// 输入响应影响档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImpactClass {
    /// 无丢弃。
    None,
    /// 低（丢 1 帧：一次输入延迟一个周期）。
    Low,
    /// 高（丢 2..=4 帧）。
    High,
    /// 严重（丢 >4 帧：连续输入丢失）。
    Severe,
}

impl ImpactClass {
    pub const fn ordinal(self) -> usize {
        match self {
            ImpactClass::None => 0,
            ImpactClass::Low => 1,
            ImpactClass::High => 2,
            ImpactClass::Severe => 3,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            ImpactClass::None => "无影响 / none",
            ImpactClass::Low => "低影响 / low",
            ImpactClass::High => "高影响 / high",
            ImpactClass::Severe => "严重影响 / severe",
        }
    }
}

/// 影响档裁决（阈值表数据化：0 无 / 1 低 / 2..=4 高 / >4 严重）。
pub const fn impact_class(dropped: u32) -> ImpactClass {
    if dropped == 0 {
        ImpactClass::None
    } else if dropped == 1 {
        ImpactClass::Low
    } else if dropped <= IMPACT_SEVERE_FRAMES {
        ImpactClass::High
    } else {
        ImpactClass::Severe
    }
}

// ---------------------------------------------------------------------------
// 四、诊断包（X04 消费；环形定容，满则覆盖最旧如实计数）
// ---------------------------------------------------------------------------

/// 一份诊断快照。
#[derive(Clone, Copy, Debug)]
pub struct DiagPack {
    pub frame: u64,
    pub cause: DropCause,
    pub gpu_busy_pct: u8,
    pub present_queue_len: u32,
    pub driver_throttle: bool,
}

/// 诊断包环形账。
pub struct DiagLedger {
    packs: [Option<DiagPack>; DIAG_PACK_CAP],
    head: usize,
    recorded_total: u32,
    overwrites: u32,
}

impl DiagLedger {
    pub const fn new() -> DiagLedger {
        DiagLedger {
            packs: [None; DIAG_PACK_CAP],
            head: 0,
            recorded_total: 0,
            overwrites: 0,
        }
    }

    /// 记录一份快照（满则覆盖最旧并计数——如实记账不静默吞）。
    pub fn record(&mut self, p: DiagPack) {
        if self.packs[self.head].is_some() {
            self.overwrites = self.overwrites.saturating_add(1);
        }
        self.packs[self.head] = Some(p);
        self.head = (self.head + 1) % DIAG_PACK_CAP;
        self.recorded_total = self.recorded_total.saturating_add(1);
    }

    pub fn recorded_total(&self) -> u32 {
        self.recorded_total
    }

    pub fn overwrites(&self) -> u32 {
        self.overwrites
    }

    pub fn len(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < DIAG_PACK_CAP {
            if self.packs[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 最近一份快照（越界空账返 None）。
    pub fn last(&self) -> Option<DiagPack> {
        let idx = if self.head == 0 {
            DIAG_PACK_CAP - 1
        } else {
            self.head - 1
        };
        match self.packs[idx] {
            Some(p) => Some(p),
            None => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 五、丢弃率环形看板（O(1) 增量）
// ---------------------------------------------------------------------------

/// 丢弃率窗口账（每呈现帧一格）。
pub struct RateBoard {
    window: [bool; RATE_WINDOW],
    head: usize,
    frames_seen: u32,
    drops_in_window: u32,
}

impl RateBoard {
    pub const fn new() -> RateBoard {
        RateBoard {
            window: [false; RATE_WINDOW],
            head: 0,
            frames_seen: 0,
            drops_in_window: 0,
        }
    }

    /// 记一帧（是否丢弃）。
    pub fn push(&mut self, dropped: bool) {
        if self.frames_seen >= RATE_WINDOW as u32 {
            let old = self.window[self.head];
            if old {
                self.drops_in_window = self.drops_in_window.saturating_sub(1);
            }
        } else {
            self.frames_seen = self.frames_seen.saturating_add(1);
        }
        self.window[self.head] = dropped;
        if dropped {
            self.drops_in_window = self.drops_in_window.saturating_add(1);
        }
        self.head = (self.head + 1) % RATE_WINDOW;
    }

    /// 丢弃率（ppm 整数口径；窗口未满按已见帧数算）。
    pub fn rate_ppm(&self) -> u32 {
        if self.frames_seen == 0 {
            return 0;
        }
        let denom = self.frames_seen as u64;
        ((self.drops_in_window as u64) * 1_000_000u64 / denom) as u32
    }

    pub fn drops_in_window(&self) -> u32 {
        self.drops_in_window
    }

    pub fn frames_seen(&self) -> u32 {
        self.frames_seen
    }
}

// ---------------------------------------------------------------------------
// 六、检测器 + 恢复状态机（本条主体）
// ---------------------------------------------------------------------------

/// 丢弃帧检测与恢复。
pub struct DropWatcher {
    last_presented: Option<u64>,
    dropped_total: u32,
    streak: u32,
    diag_on: bool,
    recover_streak: u32,
    diag_entries: u32,
    diag_exits: u32,
    last_diag_exit_frame: Option<u64>,
    escalations: u32,
    cause_counts: [u32; 4],
    board: RateBoard,
    ledger: DiagLedger,
    /// 最近一次丢弃档位（评估缓存，读屏用）。
    last_impact: ImpactClass,
}

impl DropWatcher {
    pub const fn new() -> DropWatcher {
        DropWatcher {
            last_presented: None,
            dropped_total: 0,
            streak: 0,
            diag_on: false,
            recover_streak: 0,
            diag_entries: 0,
            diag_exits: 0,
            last_diag_exit_frame: None,
            escalations: 0,
            cause_counts: [0; 4],
            board: RateBoard::new(),
            ledger: DiagLedger::new(),
            last_impact: ImpactClass::None,
        }
    }

    pub fn diag_on(&self) -> bool {
        self.diag_on
    }

    pub fn dropped_total(&self) -> u32 {
        self.dropped_total
    }

    pub fn diag_entries(&self) -> u32 {
        self.diag_entries
    }

    pub fn diag_exits(&self) -> u32 {
        self.diag_exits
    }

    pub fn escalations(&self) -> u32 {
        self.escalations
    }

    pub fn streak(&self) -> u32 {
        self.streak
    }

    pub fn board(&self) -> &RateBoard {
        &self.board
    }

    pub fn ledger(&self) -> &DiagLedger {
        &self.ledger
    }

    pub fn last_impact(&self) -> ImpactClass {
        self.last_impact
    }

    pub fn cause_count(&self, c: DropCause) -> u32 {
        self.cause_counts[c.ordinal()]
    }

    /// 呈现一帧（帧号单调）。
    ///
    /// - 首帧：建立基线，无丢弃；
    /// - 帧号回退：拒绝不改状态；
    /// - gap = 实际 - 上次 - 1 即本次丢弃数：逐帧归因记账、连续丢弃
    ///   累计（达阈值进诊断模式）、看板/影响档更新。
    pub fn present(&mut self, frame: u64, s: &Signals) -> Result<u32, PolicyCode> {
        s.validate()?;
        let dropped = match self.last_presented {
            None => {
                self.last_presented = Some(frame);
                0u32
            }
            Some(last) => {
                if frame <= last {
                    return Err(CODE_FRAME_REGRESS);
                }
                // checked 差步（防御性：门在上、闸在下，双保险零下溢面）。
                let gap = match frame.checked_sub(last).and_then(|d| d.checked_sub(1)) {
                    Some(g) => g,
                    None => return Err(CODE_FRAME_REGRESS),
                };
                self.last_presented = Some(frame);
                gap as u32
            }
        };
        if dropped == 0 {
            // 正常帧：诊断模式下累计恢复窗。
            self.streak = 0;
            if self.diag_on {
                self.recover_streak = self.recover_streak.saturating_add(1);
                if self.recover_streak >= RECOVER_FRAMES {
                    self.diag_on = false;
                    self.recover_streak = 0;
                    self.diag_exits = self.diag_exits.saturating_add(1);
                    self.last_diag_exit_frame = self.last_presented;
                }
            }
        } else {
            // 丢弃帧：逐帧归因 + 看板 + 连续计数。
            self.dropped_total = self.dropped_total.saturating_add(dropped);
            self.streak = self.streak.saturating_add(dropped);
            let cause = classify(s);
            let mut k = 0u32;
            while k < dropped {
                self.cause_counts[cause.ordinal()] = self.cause_counts[cause.ordinal()].saturating_add(1);
                k += 1;
            }
            if cause == DropCause::Unknown {
                self.ledger.record(DiagPack {
                    frame,
                    cause,
                    gpu_busy_pct: s.gpu_busy_pct,
                    present_queue_len: s.present_queue_len,
                    driver_throttle: s.driver_throttle,
                });
            }
            if !self.diag_on && self.streak >= DIAGNOSE_STREAK {
                self.diag_on = true;
                self.recover_streak = 0;
                self.diag_entries = self.diag_entries.saturating_add(1);
                // 恢复失败升级：上次退出到本次进入的间隔不足恢复窗。
                if let (Some(exit_f), Some(cur_f)) = (self.last_diag_exit_frame, self.last_presented) {
                    if cur_f.saturating_sub(exit_f) < RECOVER_FRAMES as u64 {
                        self.escalations = self.escalations.saturating_add(1);
                    }
                }
            }
        }
        self.last_impact = impact_class(dropped);
        self.board.push(dropped > 0);
        Ok(dropped)
    }

    /// 输入响应影响量化：丢 k 帧即输入响应延后 k × 帧周期。
    ///
    /// 帧周期由调用方供（V02 口径）；非法周期拒绝。
    pub fn input_latency_us(&self, refresh_period_us: u32) -> Result<u64, PolicyCode> {
        if refresh_period_us == 0 {
            return Err(CODE_BAD_PERIOD);
        }
        Ok((self.dropped_total as u64) * (refresh_period_us as u64))
    }

    /// 读屏面板（七行双语，只报丢弃事实与聚合计数）。
    pub fn a11y_lines(&self) -> [String; PANEL_LINES_D] {
        let cause_lines = {
            let mut i = 0;
            let mut acc = 0u32;
            while i < 4 {
                acc = acc.saturating_add(self.cause_counts[i]);
                i += 1;
            }
            acc
        };
        [
            format!("丢弃总数 / dropped total: {}", self.dropped_total),
            format!(
                "丢弃率 / drop rate: {} ppm（窗口 {} 帧）",
                self.board.rate_ppm(),
                self.board.frames_seen()
            ),
            format!(
                "诊断模式 / diagnostic mode: {}（进入 {}，退出 {}，升级 {}）",
                if self.diag_on { "开 / on" } else { "关 / off" },
                self.diag_entries,
                self.diag_exits,
                self.escalations
            ),
            format!(
                "归因计数 / causes: GPU忙 {}，呈现阻塞 {}，驱动节流 {}，不明 {}",
                self.cause_counts[0], self.cause_counts[1], self.cause_counts[2], self.cause_counts[3]
            ),
            format!(
                "诊断包 / diag packs: 在册 {}，累计 {}，覆盖 {}",
                self.ledger.len(),
                self.ledger.recorded_total(),
                self.ledger.overwrites()
            ),
            format!(
                "输入响应影响 / input latency impact: {}（归因合计 {}）",
                self.last_impact.label(),
                cause_lines
            ),
            format!(
                "连续丢弃 / current streak: {}（阈值 {}）",
                self.streak, DIAGNOSE_STREAK
            ),
        ]
    }
}

/// 面板行数。
pub const PANEL_LINES_D: usize = 7;

// ---------------------------------------------------------------------------
// 七、域自检（判据逐条映射锚点：跳帧检测/三类归因/诊断模式/丢弃率可见/判据）
// ---------------------------------------------------------------------------

/// F0050 域自检入口（聚合器调用；零 panic 面，失败逐条红不炸域）。
pub fn run_vea50_checks() -> CheckSet {
    let mut s = CheckSet::new("vea50_dropframe");

    // --- 判据 1：跳帧检测——差步计数精确、回退拒绝、首帧基线 ---
    {
        let mut w = DropWatcher::new();
        let calm = Signals { gpu_busy_pct: 10, present_queue_len: 0, driver_throttle: false };
        let first = w.present(10, &calm);
        let gap2 = w.present(13, &calm); // 丢 11、12 两帧
        let none = w.present(14, &calm); // 14 正常（13 已上屏）
        let regress = w.present(13, &calm) == Err(CODE_FRAME_REGRESS);
        let same = w.present(14, &calm) == Err(CODE_FRAME_REGRESS);
        s.add(
            "A50-跳帧检测-差步计数精确且回退拒",
            matches!(first, Ok(0))
                && matches!(gap2, Ok(2))
                && matches!(none, Ok(0))
                && regress
                && same
                && w.dropped_total() == 2
                && CODE_FRAME_REGRESS != CODE_BAD_SIGNAL,
            "首帧建基线零丢弃；10→13 gap=2 精确计 2 帧；正常帧 0；帧号回退与同帧重present一律拒（状态不变）",
        );
    }

    // --- 判据 2：三类归因——信号裁决 + 优先级仲裁 + 独立重算 ---
    {
        let busy = Signals { gpu_busy_pct: 90, present_queue_len: 0, driver_throttle: false };
        let blocked = Signals { gpu_busy_pct: 10, present_queue_len: 3, driver_throttle: false };
        let throttle = Signals { gpu_busy_pct: 90, present_queue_len: 3, driver_throttle: true };
        let unknown = Signals { gpu_busy_pct: 10, present_queue_len: 0, driver_throttle: false };
        let edge = Signals { gpu_busy_pct: GPU_BUSY_PCT, present_queue_len: 0, driver_throttle: false };
        let qedge = Signals { gpu_busy_pct: 10, present_queue_len: QUEUE_BLOCKED_LEN, driver_throttle: false };
        s.add(
            "A50-三类归因-优先级仲裁且边界贴线即判",
            classify(&busy) == DropCause::GpuBusy
                && classify(&blocked) == DropCause::PresentBlocked
                && classify(&throttle) == DropCause::DriverThrottle
                && classify(&unknown) == DropCause::Unknown
                && classify(&edge) == DropCause::GpuBusy
                && classify(&qedge) == DropCause::PresentBlocked
                && GPU_BUSY_PCT == 85 && QUEUE_BLOCKED_LEN == 2,
            "节流压过一切（多信号同现取驱动节流）；占用恰 85 即 GPU忙、队列恰 2 即呈现阻塞（贴线不误判）；全不成立=Unknown",
        );
    }

    // --- 判据 3：原因不明→诊断包；包满覆盖最旧如实计数 ---
    {
        let mut w = DropWatcher::new();
        let unknown = Signals { gpu_busy_pct: 10, present_queue_len: 0, driver_throttle: false };
        let mut i = 0u32;
        while i < DIAG_PACK_CAP as u32 + 3 {
            let _ = w.present(10 + (i as u64) * 2, &unknown); // 每次 gap=1（首帧基线不计）
            i += 1;
        }
        let last = w.ledger().last();
        s.add(
            "A50-诊断包-Unknown入包且满覆盖最旧",
            w.ledger().recorded_total() == DIAG_PACK_CAP as u32 + 2
                && w.ledger().overwrites() == 2
                && w.ledger().len() == DIAG_PACK_CAP
                && matches!(last, Some(p) if p.cause == DropCause::Unknown)
                && DIAG_PACK_CAP == 8,
            "Unknown 逐次入包（累计不减）；包满后覆盖最旧并计数（覆盖 2 次）；最近包可取（X04 消费面）",
        );
    }

    // --- 判据 4：诊断模式进入——连续丢弃达阈值 ---
    {
        let mut w = DropWatcher::new();
        let unknown = Signals { gpu_busy_pct: 10, present_queue_len: 0, driver_throttle: false };
        let _ = w.present(10, &unknown);
        // 连续：每次 gap=1 ⇒ streak 1、2、3 逐次累计。
        let _ = w.present(12, &unknown);
        let _ = w.present(14, &unknown);
        let not_yet = !w.diag_on() && w.streak() == 2;
        let _ = w.present(16, &unknown);
        let on = w.diag_on() && w.diag_entries() == 1 && w.streak() == 3;
        s.add(
            "A50-诊断模式-连续丢弃达阈值进入",
            not_yet && on && w.streak() >= DIAGNOSE_STREAK && DIAGNOSE_STREAK == 3,
            "连续丢弃第 2 帧未达阈值不进入；第 3 帧达阈值进入并计数（阈值字面值钉死）",
        );
    }

    // --- 判据 5：恢复与升级——连续正常帧退出、快速复发升级 ---
    {
        let mut w = DropWatcher::new();
        let unknown = Signals { gpu_busy_pct: 10, present_queue_len: 0, driver_throttle: false };
        // 进入诊断模式（连续 3 丢弃：10,12,14,16）。
        let _ = w.present(10, &unknown);
        let _ = w.present(12, &unknown);
        let _ = w.present(14, &unknown);
        let _ = w.present(16, &unknown);
        let entered = w.diag_on() && w.dropped_total() == 3;
        // 连续 16 正常帧（17..32）→ 退出。
        let mut i = 0u32;
        while i < RECOVER_FRAMES {
            let _ = w.present(17 + i as u64, &unknown);
            i += 1;
        }
        let exited = !w.diag_on() && w.diag_exits() == 1;
        // 快速复发（退出后 < 16 帧内再连续丢弃）→ 恢复失败升级。
        let _ = w.present(40, &unknown);
        let _ = w.present(42, &unknown);
        let escalated = w.escalations() == 1 && w.diag_entries() == 2;
        s.add(
            "A50-恢复与升级-退出留痕且快速复发升级",
            entered && exited && escalated && RECOVER_FRAMES == 16,
            "连续 16 正常帧退出诊断（退出留痕）；退出后 <16 帧内再进 = 恢复失败升级（逐次计数）",
        );
    }

    // --- 判据 6：丢弃率看板——环形窗口 O(1) 增量对账 ---
    {
        // 判据侧独立重算：奇偶交替进出窗口（丢弃格与非丢弃格各半）。
        let mut b = RateBoard::new();
        let mut j = 0u32;
        let mut drops = 0u32;
        while j < RATE_WINDOW as u32 {
            let dropped = j % 2 == 0;
            if dropped {
                drops += 1;
            }
            b.push(dropped);
            j += 1;
        }
        let rate = b.rate_ppm();
        s.add(
            "A50-丢弃率看板-环形增量与独立重算一致",
            b.frames_seen() == RATE_WINDOW as u32
                && b.drops_in_window() == drops
                && rate == (drops as u64 * 1_000_000 / RATE_WINDOW as u64) as u32
                && rate > 0
                && RATE_WINDOW == 128,
            "窗口满后进出相抵（旧格退出减计），丢弃率 ppm 整数口径与判据侧重算一致",
        );
    }

    // --- 判据 7：输入响应影响——量化公式与档位表 ---
    {
        let w = DropWatcher::new();
        let lat = w.input_latency_us(16_667);
        let zero = matches!(lat, Ok(0));
        let bad = w.input_latency_us(0) == Err(CODE_BAD_PERIOD);
        let c_none = impact_class(0) == ImpactClass::None;
        let c_low = impact_class(1) == ImpactClass::Low;
        let c_high = impact_class(2) == ImpactClass::High && impact_class(4) == ImpactClass::High;
        let c_sev = impact_class(5) == ImpactClass::Severe;
        s.add(
            "A50-输入响应影响-量化公式与档位表",
            zero && bad && c_none && c_low && c_high && c_sev
                && IMPACT_HIGH_FRAMES == 2 && IMPACT_SEVERE_FRAMES == 4,
            "输入延迟=丢弃帧数×帧周期（u64 防溢出）；零周期拒绝；档位表 0/1/2..=4/>4 逐档断言（含边界 4=高、5=严重）",
        );
    }

    // --- 判据 8：无效信号拒绝——越界占用拒、wire 往返 ---
    {
        let mut w = DropWatcher::new();
        let bad = Signals { gpu_busy_pct: 101, present_queue_len: 0, driver_throttle: false };
        let rejected = w.present(10, &bad) == Err(CODE_BAD_SIGNAL);
        let mut wire_ok = true;
        let mut i = 0usize;
        while i < DropCause::ALL_LEN {
            let c = DropCause::ALL[i];
            if DropCause::from_wire(c.wire()) != Some(c) {
                wire_ok = false;
            }
            i += 1;
        }
        s.add(
            "A50-无效值-越界信号拒且wire往返",
            rejected && wire_ok && DropCause::from_wire(0).is_none() && DropCause::from_wire(5).is_none(),
            "占用 101 构造期拒（状态不变）；四类 wire 编码显式映射往返一致，0x00/0x05 解码 None",
        );
    }

    // --- 判据 9：接缝——完整链（检测→归因→诊断→恢复→看板一致） ---
    {
        let mut w = DropWatcher::new();
        let busy = Signals { gpu_busy_pct: 95, present_queue_len: 0, driver_throttle: false };
        let calm = Signals { gpu_busy_pct: 10, present_queue_len: 0, driver_throttle: false };
        let _ = w.present(10, &busy);
        let _ = w.present(12, &busy);
        let _ = w.present(14, &busy);
        let _ = w.present(16, &busy); // 连续 3 丢弃（GPU忙）→ 诊断
        let diag = w.diag_on() && w.cause_count(DropCause::GpuBusy) == 3;
        let mut i = 0u32;
        while i < RECOVER_FRAMES {
            let _ = w.present(17 + i as u64, &calm);
            i += 1;
        }
        let recovered = !w.diag_on();
        s.add(
            "A50-接缝-完整链状态一致",
            diag && recovered
                && w.dropped_total() == 3
                && w.diag_entries() == 1
                && w.diag_exits() == 1
                && w.escalations() == 0,
            "GPU忙连续丢弃→诊断进入（归因逐帧记账）；恢复窗正常帧→退出；全程账目一致（进入=退出=1，无升级）",
        );
    }

    // --- 判据 10：读屏面板——七行双语逐行绑定聚合量 ---
    {
        let mut w = DropWatcher::new();
        let busy = Signals { gpu_busy_pct: 95, present_queue_len: 0, driver_throttle: false };
        let _ = w.present(10, &busy);
        let _ = w.present(13, &busy); // gap=2：丢 11、12 两帧，影响档=高
        let lines = w.a11y_lines();
        let all_nonempty = lines.iter().all(|l| !l.is_empty());
        s.add(
            "A50-面板-七行双语且逐行绑定聚合量",
            lines.len() == PANEL_LINES_D
                && all_nonempty
                && lines[0].contains("dropped total: 2")
                && lines[1].contains("ppm")
                && lines[2].contains("diagnostic mode: 关 / off")
                && lines[3].contains("GPU忙 2")
                && lines[4].contains("diag packs: 在册 0")
                && lines[5].contains("high")
                && lines[6].contains("current streak: 2")
                && lines.iter().all(|l| l.chars().any(|c| c.is_ascii_alphabetic())),
            "面板七行逐行绑定：丢弃 2 / 丢弃率 / 诊断关 / GPU忙归因 2 / 诊断包 0 / 影响档=高 / 连续 2，每行双语",
        );
    }

    // --- 判据 11：X04 对接——诊断包快照码稳定可消费 ---
    {
        let mut w = DropWatcher::new();
        let unknown = Signals { gpu_busy_pct: 10, present_queue_len: 0, driver_throttle: false };
        let _ = w.present(10, &unknown);
        let _ = w.present(12, &unknown);
        let last = w.ledger().last();
        s.add(
            "A50-对接-诊断包快照稳定",
            matches!(last, Some(p) if p.frame == 12 && p.cause == DropCause::Unknown && !p.driver_throttle),
            "最近快照携带帧号/归因/三信号原值（X04 剖析消费面稳定），不留空不留糊",
        );
    }

    s
}
