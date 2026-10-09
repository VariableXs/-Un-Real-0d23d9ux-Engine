//! VE-F0231 · Intel 频率与功耗遥测（VE-B 域 · VE-B Intel 驱动矩阵 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0231`
//!
//! **职责定位（锚点原文）**：Intel 频率与功耗遥测——RPn/RPe/RP0 频率档位与
//! 实际频率采集、封装功耗 RAPL **只读周期采样**，入遥测总线**受 F0096 预算治理**；
//! **采样断档标记缺测不插值**；**异常值如实记录并标记可疑**供漂移检测参考。
//!
//! **判据（锚点原文）**：只读采样、缺测不插值、预算治理、可疑标记、判据。
//!
//! **数据结构（锚点原文）**：遥测样本（频率×功耗×时间戳）；档位边界表；
//! 可疑标记。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 采样失败 → **缺测标记** | [`FreqPowerTelemetry::poll`] 在到期槽位读不到频率主轴（`freq_mhz=None`）时推入缺测样本（`freq=None, power=None`）——**无任何编造值**：不插值、不补零、不沿用上一采样值 |
//! | RAPL 不可用 → **仅频率维度并标注** | [`FreqPowerTelemetry::set_rapl_available`] 回填只读可用性事实；不可用时功耗维度合法为 `None`，最近诊断码置 [`CODE_RAPL_UNAVAILABLE`]（标注不是静默降级——静默会让下游把「没有功耗」读成「功耗为零」） |
//! | 异常值 → **如实记录标记可疑不丢弃** | 越出档位边界表总包络的频率、越出合理包功耗区间的功耗，**照原值入总线**并打 `suspicious` 标记（[`CODE_SUSPICIOUS_FREQ`]/[`CODE_SUSPICIOUS_POWER`]）——丢弃异常值等于销毁 F0100 漂移检测的事实来源 |
//!
//! **性能逐项分解（锚点原文）**：采样 O(1)；入总线 O(1)；零插值成本。
//! poll 的断档补标有 [`CATCHUP_CAP`] 硬上界（超出的跳过槽位只如实计数、不再
//! 逐槽补标——补标本身也是成本），档位归档 [`classify_freq`] 是三格线性扫，
//! 总线 push 是定容环形写——全部常数时间、零堆分配、零墙钟。
//!
//! **跨批对接点（锚点原文）**：上游 F0091/F0096；下游 F0100 漂移检测与
//! F0239 调试通道。
//! - **F0091**（能力探测）：RAPL 只读可用性事实的唯一权威——本条只回填持有
//!   （[`FreqPowerTelemetry::set_rapl_available`]），不在本条内裁决；
//! - **F0096**（遥测预算治理）：采样间隔的唯一权威——本条只按回填的间隔
//!   到期采样（[`FreqPowerTelemetry::set_budget`]），遥测不许自扰主路；
//! - **F0100**（漂移检测）：消费 `suspicious` 标记与缺测事实（本条只生产
//!   事实不做裁决）；**F0239**（调试通道）：样本经总线出芯。
//!
//! **无障碍与隐私（锚点原文）**：无直接无障碍面——本条是驱动侧遥测源，
//! 不直接面向用户呈现（遥测面板在 F0100/F0239 侧）。
//!
//! ## 设计要点（为什么这样写）
//!
//! - **只读红线是类型级的不变量**：频率与功耗全部由 [`poll`] 参数注入
//!   （调用方=寄存器读取方），本模块不存在任何写寄存器路径；入总线的值
//!   **逐位等于注入值**（无单位换算、无钳制、无修数）——「只读」不是注释
//!   承诺而是判据钉死的可运行事实。
//! - **缺测与「值为零」是两种病**：缺测样本频率功耗**双 None**；而功耗为
//!   0 mW 是**异常值**（活体封装不可能零功耗）照原值入账并标可疑。把缺测
//!   记成零，会让漂移检测把「读不到」学成「不耗电」——本案最贵的谎言。
//! - **频率是遥测主轴**：`(freq=None, power=Some)` 的矛盾态（频率寄存器读不
//!   到而功耗读得到）整条按缺测标记——无频率的功耗样本无法供 F0100 做
//!   频率-功耗关联，留着比缺测更坏（断链的半样本会被当成完整样本统计）。
//! - **断档补标有上界，超出的只计数**：调度抖动可能导致到期槽位被整段跳过
//!   （tick 跳跃）。补标逐槽缺测最多 [`CATCHUP_CAP`] 条，剩余跳过槽位如实
//!   计入 `gaps`——「标 983 个缺测」与「标 16 个缺测+账上记 983」对下游
//!   是同一事实，但后者 O(1) 有界；诚实不等于无限成本。
//! - **预算内不采样**：未到期的 poll 是 `Idle`——不入总线、不动计数。
//!   遥测的存在意义是观测，观测动作本身不许成为被观测系统的负载源。
//! - **tick 只进不退**：tick 回退的 poll 显性拒绝（[`CODE_BAD_REQUEST`]）
//!   并**不清账**——相位错是调用方 bug，丢弃合法在途排程等于让调用方的
//!   bug 摧毁本条的时间基准。
//! - **零 panic 面**：查表走 `get`/match、计数全 `saturating_add`、无
//!   unwrap/expect——判据区同样约束。

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0x4Fxx，独占——0x4E 及以下已占，全仓 grep 零占用后选定）
// ---------------------------------------------------------------------------

/// 非法请求（tick 回退相位错 / 预算间隔为零）。
pub const CODE_BAD_REQUEST: u16 = 0x4F01;
/// 采样失败（频率主轴读不到）：缺测标记已推入，不插值不编值。
pub const CODE_SAMPLE_FAILED: u16 = 0x4F02;
/// RAPL 不可用：仅频率维度并标注（功耗维度合法 None）。
pub const CODE_RAPL_UNAVAILABLE: u16 = 0x4F03;
/// 频率越出档位边界表总包络：如实记录并标记可疑。
pub const CODE_SUSPICIOUS_FREQ: u16 = 0x4F04;
/// 功耗越出合理包功耗区间：如实记录并标记可疑。
pub const CODE_SUSPICIOUS_POWER: u16 = 0x4F05;
/// 预算拒绝（F0096 间隔为零=无预算可给）。
pub const CODE_BUDGET_EXCEEDED: u16 = 0x4F06;

/// 本域诊断码全集（判据对账：互异 + 独占 0x4F 段）。
pub const CODES: [u16; 6] = [
    CODE_BAD_REQUEST,
    CODE_SAMPLE_FAILED,
    CODE_RAPL_UNAVAILABLE,
    CODE_SUSPICIOUS_FREQ,
    CODE_SUSPICIOUS_POWER,
    CODE_BUDGET_EXCEEDED,
];

/// 人话说明（后果 + 下一步，不能只说「失败」；未知码有兜底不 panic）。
pub const fn explain(code: u16) -> &'static str {
    match code {
        CODE_BAD_REQUEST => "非法请求（tick 回退/间隔为零）：检查调用方时序与 F0096 预算参数后重放",
        CODE_SAMPLE_FAILED => "采样失败已标记缺测：本槽无任何值（不插值），按注入失败的频率寄存器排查",
        CODE_RAPL_UNAVAILABLE => "RAPL 不可用：本回合仅频率维度（功耗维度为 None 非为零），可用性事实以 F0091 为准",
        CODE_SUSPICIOUS_FREQ => "频率越出档位边界表：已照原值入账并标可疑，供 F0100 漂移检测参考",
        CODE_SUSPICIOUS_POWER => "功耗越出合理包功耗区间：已照原值入账并标可疑，供 F0100 漂移检测参考",
        CODE_BUDGET_EXCEEDED => "预算拒绝：F0096 采样间隔为零（无预算可给），回填合法间隔后重试",
        // 兜底：码外值给人话而不是崩掉。
        _ => "未知频率功耗遥测诊断码（未登记）",
    }
}

// ---------------------------------------------------------------------------
// 二、频率档位边界表（RPn / RPe / RP0——标称值，表驱动可实测修正）
// ---------------------------------------------------------------------------

/// 频率档位（RPn=最低 P 态 / RPe=标称基频 / RP0=最高睿频）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PStateLevel {
    /// 最低档（RPn：能效档）。
    RPn,
    /// 标称档（RPe：基频附近）。
    RPe,
    /// 最高档（RP0：最大睿频）。
    RP0,
}

/// 在册档位表（判据对账基准：恰三档）。
pub const LEVELS: [PStateLevel; 3] = [PStateLevel::RPn, PStateLevel::RPe, PStateLevel::RP0];

impl PStateLevel {
    /// 序号（查表用；与 `from_ordinal` 互为逆）。
    pub const fn ordinal(self) -> usize {
        match self {
            PStateLevel::RPn => 0,
            PStateLevel::RPe => 1,
            PStateLevel::RP0 => 2,
        }
    }

    /// 由序号还原（越界 `None`，不留默认兜底）。
    pub const fn from_ordinal(i: usize) -> Option<PStateLevel> {
        match i {
            0 => Some(PStateLevel::RPn),
            1 => Some(PStateLevel::RPe),
            2 => Some(PStateLevel::RP0),
            _ => None,
        }
    }

    /// 线上码（显式映射，禁 `as u8` 直转——枚举判别值与线约可能分叉）。
    pub const fn wire(self) -> u8 {
        match self {
            PStateLevel::RPn => 0x01,
            PStateLevel::RPe => 0x02,
            PStateLevel::RP0 => 0x03,
        }
    }

    /// 由线上码还原。
    pub const fn from_wire(w: u8) -> Option<PStateLevel> {
        match w {
            0x01 => Some(PStateLevel::RPn),
            0x02 => Some(PStateLevel::RPe),
            0x03 => Some(PStateLevel::RP0),
            _ => None,
        }
    }

    /// 标签（遥测审计与调试通道用）。
    pub const fn label(self) -> &'static str {
        match self {
            PStateLevel::RPn => "RPn 最低档 / RPn min",
            PStateLevel::RPe => "RPe 标称档 / RPe nominal",
            PStateLevel::RP0 => "RP0 最高睿频 / RP0 max turbo",
        }
    }

    /// 档位下界（MHz；标称表值）。
    pub const fn min_mhz(self) -> u16 {
        match self {
            PStateLevel::RPn => 400,
            PStateLevel::RPe => 801,
            PStateLevel::RP0 => 2801,
        }
    }

    /// 档位上界（MHz；标称表值）。
    pub const fn max_mhz(self) -> u16 {
        match self {
            PStateLevel::RPn => 800,
            PStateLevel::RPe => 2800,
            PStateLevel::RP0 => 5800,
        }
    }
}

/// 频率合理区间总包络（= RPn.min ..= RP0.max；越出即可疑）。
pub const FREQ_MIN_MHZ: u16 = 400;
/// 频率合理区间上界（= RP0.max）。
pub const FREQ_MAX_MHZ: u16 = 5800;

/// 按实际频率归档位（O(1) 三格扫；越界 `None` = 可疑频率）。
///
/// 边界表**非重叠全覆盖**（RPn.max+1 == RPe.min、RPe.max+1 == RP0.min——
/// 判据钉死），故任一在册频率恰归一档，无频率落空洞。
pub const fn classify_freq(mhz: u16) -> Option<PStateLevel> {
    let mut i = 0;
    while i < LEVELS.len() {
        let l = LEVELS[i];
        if mhz >= l.min_mhz() && mhz <= l.max_mhz() {
            return Some(l);
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 三、遥测样本（锚点数据结构：频率×功耗×时间戳 + 可疑标记）
// ---------------------------------------------------------------------------

/// 遥测样本（锚点数据结构：频率×功耗×时间戳；可疑标记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelemetrySample {
    /// 实际频率（MHz）；`None` = 缺测（频率主轴缺失）。
    pub freq_mhz: Option<u16>,
    /// 封装功耗（mW）；`None` = RAPL 不可用或缺测（**不是零**）。
    pub power_mw: Option<u32>,
    /// 逻辑时间戳（tick；不用墙钟——对拍可复现）。
    pub tick: u64,
    /// 可疑标记（异常值如实记录不丢弃，供 F0100 漂移检测参考）。
    pub suspicious: bool,
}

impl TelemetrySample {
    /// 空样本（缺测哨兵：无频率无功耗）。
    pub const EMPTY: TelemetrySample = TelemetrySample {
        freq_mhz: None,
        power_mw: None,
        tick: 0,
        suspicious: false,
    };

    /// 是否缺测（频率主轴缺失）。
    pub const fn is_missing(&self) -> bool {
        self.freq_mhz.is_none()
    }

    /// 归档位（缺测 `None`；在册但越界的可疑频率同样 `None`——归档失败是
    /// 可疑的信号不是默认档）。
    pub const fn level(&self) -> Option<PStateLevel> {
        match self.freq_mhz {
            Some(m) => classify_freq(m),
            None => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 四、遥测总线（定容环形——满则逐最老覆盖，覆盖如实计数）
// ---------------------------------------------------------------------------

/// 遥测总线容量（定容——no_std 零堆）。
pub const BUS_CAP: usize = 64;

/// 定容环形遥测总线。
#[derive(Clone, Copy, Debug)]
pub struct TelemetryBus {
    entries: [TelemetrySample; BUS_CAP],
    /// 下一个写入位。
    head: usize,
    /// 历史写入总数（可超 `BUS_CAP`——覆盖语义）。
    total: usize,
    /// 覆盖最旧次数（满册后每次写入 +1，如实计数）。
    overwrites: u32,
}

impl TelemetryBus {
    /// 空总线。
    pub const fn new() -> TelemetryBus {
        TelemetryBus {
            entries: [TelemetrySample::EMPTY; BUS_CAP],
            head: 0,
            total: 0,
            overwrites: 0,
        }
    }

    /// 推入一条（满则覆盖最旧并计数）。
    pub fn push(&mut self, s: TelemetrySample) {
        if self.total >= BUS_CAP {
            self.overwrites = self.overwrites.saturating_add(1);
        }
        self.entries[self.head] = s;
        self.head = (self.head + 1) % BUS_CAP;
        self.total = self.total.saturating_add(1);
    }

    /// 在册条数（≤ `BUS_CAP`）。
    pub fn len(&self) -> usize {
        if self.total < BUS_CAP {
            self.total
        } else {
            BUS_CAP
        }
    }

    /// 历史推入总数。
    pub fn total(&self) -> usize {
        self.total
    }

    /// 覆盖次数。
    pub const fn overwrites(&self) -> u32 {
        self.overwrites
    }

    /// 最新一条（空总线 `None`）。
    pub fn latest(&self) -> Option<TelemetrySample> {
        if self.total == 0 {
            return None;
        }
        let idx = (self.head + BUS_CAP - 1) % BUS_CAP;
        Some(self.entries[idx])
    }

    /// 按下标读（0 = 最旧仍在册；越界 `None`）。
    pub fn get(&self, i: usize) -> Option<TelemetrySample> {
        let len = self.len();
        if i >= len {
            return None;
        }
        let start = (self.head + BUS_CAP - len) % BUS_CAP;
        let idx = (start + i) % BUS_CAP;
        Some(self.entries[idx])
    }

    /// 独立统计（判据侧重算用，不复用管理器计数）：(缺测数, 可疑数)。
    pub fn tally(&self) -> (usize, usize) {
        let mut missing = 0usize;
        let mut suspicious = 0usize;
        let mut i = 0;
        while i < self.len() {
            match self.get(i) {
                Some(s) => {
                    if s.is_missing() {
                        missing += 1;
                    }
                    if s.suspicious {
                        suspicious += 1;
                    }
                }
                None => break,
            }
            i += 1;
        }
        (missing, suspicious)
    }
}

// ---------------------------------------------------------------------------
// 五、预算治理的采样管理器（锚点「数据结构：切换管理」→ 采样管理）
// ---------------------------------------------------------------------------

/// F0096 预算采样间隔下界（tick；0 = 无预算可给，显性拒绝）。
pub const MIN_INTERVAL_TICKS: u16 = 1;
/// 断档补标上限（O(1) 有界：超出的跳过槽位只如实计数 `gaps`）。
pub const CATCHUP_CAP: u32 = 16;
/// 合理包功耗下界（mW；0 = 活体封装不可能零功耗，即可疑）。
pub const POWER_MIN_MW: u32 = 1;
/// 合理包功耗上界（mW；标称 250W 封装上限，表驱动可修正）。
pub const POWER_MAX_MW: u32 = 250_000;

/// poll 结果（拒绝与降级路径全部显性分码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PollOutcome {
    /// 未到期（预算内不采样——遥测不许自扰主路）。
    Idle,
    /// 有效采样（`suspicious` = 本样本是否带可疑标记）。
    Sampled {
        /// 是否标记可疑（异常值：越界频率/功耗，或声明可用却缺功耗）。
        suspicious: bool,
    },
    /// 缺测标记（采样失败或断档补标——无任何编造值）。
    Missing,
    /// 拒绝（tick 回退相位错；不清账，调用方时序 bug）。
    Rejected(u16),
}

/// Intel 频率与功耗遥测管理器（锚点数据结构）。
///
/// 不变式：`interval_ticks ≥ 1`（构造即净化，`set_budget` 运行时显性拒绝
/// 零间隔）；`due_tick == 0` 表示未排程（首 poll 只立到期点不采样）；
/// `tick` 单调不回退（回退即拒）。
#[derive(Clone, Copy, Debug)]
pub struct FreqPowerTelemetry {
    /// F0096 预算回填的采样间隔（tick；≥ 1）。
    interval_ticks: u16,
    /// RAPL 只读可用性事实（F0091 探测回填；本条不裁决只如实持有）。
    rapl_available: bool,
    /// 定容环形遥测总线。
    bus: TelemetryBus,
    /// 下次采样到期 tick（0 = 未排程）。
    due_tick: u64,
    /// 最近一次 poll 的 tick（相位错检测基准）。
    last_tick: u64,
    /// 有效样本数（频率主轴在册的采样）。
    samples: u32,
    /// 缺测标记数（采样失败 + 断档补标）。
    missing: u32,
    /// 可疑标记数。
    suspicious: u32,
    /// 断档跳过槽位数（补标上限 [`CATCHUP_CAP`] 之外的如实计数）。
    gaps: u32,
    /// 预算拒绝数（`set_budget(0)`）。
    budget_rejects: u32,
    /// 最近诊断码（0 = 无）。
    last_code: u16,
}

impl FreqPowerTelemetry {
    /// 新管理器（RAPL 可用性事实 + 初期间隔；间隔构造时净化到 ≥1）。
    pub const fn new(rapl_available: bool, interval_ticks: u16) -> FreqPowerTelemetry {
        FreqPowerTelemetry {
            interval_ticks: if interval_ticks < MIN_INTERVAL_TICKS {
                MIN_INTERVAL_TICKS
            } else {
                interval_ticks
            },
            rapl_available,
            bus: TelemetryBus::new(),
            due_tick: 0,
            last_tick: 0,
            samples: 0,
            missing: 0,
            suspicious: 0,
            gaps: 0,
            budget_rejects: 0,
            last_code: 0,
        }
    }

    /// 现役采样间隔（tick）。
    pub const fn interval_ticks(&self) -> u16 {
        self.interval_ticks
    }

    /// RAPL 可用性事实（只读视图）。
    pub const fn rapl_available(&self) -> bool {
        self.rapl_available
    }

    /// 下次采样到期 tick（0 = 未排程）。
    pub const fn due_tick(&self) -> u64 {
        self.due_tick
    }

    /// 遥测总线（只读视图）。
    pub const fn bus(&self) -> &TelemetryBus {
        &self.bus
    }

    /// 聚合计数（有效样本 / 缺测 / 可疑 / 断档跳过槽 / 预算拒绝）。
    pub const fn counters(&self) -> (u32, u32, u32, u32, u32) {
        (
            self.samples,
            self.missing,
            self.suspicious,
            self.gaps,
            self.budget_rejects,
        )
    }

    /// 最近一次诊断码（0 = 无）。
    pub const fn last_code(&self) -> u16 {
        self.last_code
    }

    /// F0091 回填 RAPL 只读可用性事实（唯一权威在 F0091，本条只持有）。
    ///
    /// 置不可用时留 [`CODE_RAPL_UNAVAILABLE`] 痕迹——「没有功耗数据」
    /// 与「功耗为零」必须可分辨。
    pub fn set_rapl_available(&mut self, ok: bool) {
        self.rapl_available = ok;
        if !ok {
            self.last_code = CODE_RAPL_UNAVAILABLE;
        }
    }

    /// F0096 回填采样预算（间隔 tick 数；零 = 无预算可给，显性拒绝）。
    pub fn set_budget(&mut self, interval_ticks: u16) -> Result<(), u16> {
        if interval_ticks < MIN_INTERVAL_TICKS {
            self.budget_rejects = self.budget_rejects.saturating_add(1);
            self.last_code = CODE_BUDGET_EXCEEDED;
            return Err(CODE_BUDGET_EXCEEDED);
        }
        self.interval_ticks = interval_ticks;
        Ok(())
    }

    /// 周期采样入口（驱动侧每 tick 调用；注入频率与功耗读数）。
    ///
    /// 时序契约（全部显性分码，无静默路径）：
    /// 1. `tick` 回退 → [`PollOutcome::Rejected`]（相位错，不清账）；
    /// 2. 首 poll → 排程到期点（[`PollOutcome::Idle`]，首 tick 不采样）；
    /// 3. 未到期 → [`PollOutcome::Idle`]（预算内不扰动）；
    /// 4. 断档（`tick` 跃过到期槽位）→ 逐槽补标缺测（上限
    ///    [`CATCHUP_CAP`]），超出只如实计入 `gaps`（不插值）；
    /// 5. 到期 → 按读数走 [`Self::sample_now`]（有效 / 缺测 / 可疑）。
    pub fn poll(
        &mut self,
        tick: u64,
        freq_mhz: Option<u16>,
        power_mw: Option<u32>,
    ) -> PollOutcome {
        // 1. 相位错：tick 回退拒绝（时间不可倒流；已排程过的管理器才校验）。
        if self.due_tick != 0 && tick < self.last_tick {
            self.last_code = CODE_BAD_REQUEST;
            return PollOutcome::Rejected(CODE_BAD_REQUEST);
        }
        self.last_tick = tick;
        // 2. 首 poll：立到期点不采样（没有「第 0 tick 就必须有样本」的道理）。
        if self.due_tick == 0 {
            self.due_tick = tick.saturating_add(self.interval_ticks as u64);
            return PollOutcome::Idle;
        }
        // 3. 预算内未到期：不采样、不入总线、不动计数。
        if tick < self.due_tick {
            return PollOutcome::Idle;
        }
        // 4. 断档补标：被跳过的到期槽位逐槽缺测（上限内）。
        // 标记的时间戳是**被跳过槽位的到期 tick**而非本次 poll 的 tick——
        // 缺测事实的时间必须指向缺失的那一刻，不能指向发现缺失的那一刻。
        let mut catchup: u32 = 0;
        while self.due_tick < tick && catchup < CATCHUP_CAP {
            let slot = self.due_tick;
            self.push_missing(slot);
            self.due_tick = self.due_tick.saturating_add(self.interval_ticks as u64);
            catchup += 1;
        }
        // 4b. 上限外的跳过槽位：如实计数后对齐到期点。
        if self.due_tick < tick {
            let remaining = (tick - self.due_tick) / (self.interval_ticks as u64);
            self.gaps = self.gaps.saturating_add(remaining as u32);
            self.due_tick = tick;
        }
        // 5. 到期采样。
        let outcome = self.sample_now(tick, freq_mhz, power_mw);
        self.due_tick = tick.saturating_add(self.interval_ticks as u64);
        outcome
    }

    /// 推入缺测样本（双 None——无任何编造值）。
    fn push_missing(&mut self, tick: u64) {
        let sample = TelemetrySample {
            freq_mhz: None,
            power_mw: None,
            tick,
            suspicious: false,
        };
        self.bus.push(sample);
        self.missing = self.missing.saturating_add(1);
    }

    /// 到期槽位采样（读不到频率主轴 → 缺测；读得到 → 归档 + 可疑判定）。
    fn sample_now(
        &mut self,
        tick: u64,
        freq_mhz: Option<u16>,
        power_mw: Option<u32>,
    ) -> PollOutcome {
        match (freq_mhz, power_mw) {
            // 采样失败：缺测标记（不插值、不补零、不沿用上一值）。
            (None, None) => {
                self.push_missing(tick);
                self.last_code = CODE_SAMPLE_FAILED;
                PollOutcome::Missing
            }
            // 频率主轴缺失而功耗读得到：矛盾态整条缺测——无频率的功耗
            // 样本无法供 F0100 做频率-功耗关联，断链半样本比缺测更坏。
            (None, Some(_)) => {
                self.push_missing(tick);
                self.last_code = CODE_SAMPLE_FAILED;
                PollOutcome::Missing
            }
            (Some(mhz), pw) => {
                // 频率维度：越出档位边界表总包络即可疑。
                let freq_suspicious = classify_freq(mhz).is_none();
                // 功耗维度：
                // - RAPL 不可用（F0091 事实）→ None 合法（仅频率维度）；
                // - 声明可用却读不到 → 可疑（读失败不是不可用）；
                // - 读到的值越出合理包功耗区间 → 可疑（0 或超上界）。
                let power_suspicious = match pw {
                    None => self.rapl_available,
                    Some(mw) => mw < POWER_MIN_MW || mw > POWER_MAX_MW,
                };
                let suspicious = freq_suspicious || power_suspicious;
                // 只读语义：注入值逐位入账（无换算、无钳制、无修数）。
                let sample = TelemetrySample {
                    freq_mhz: Some(mhz),
                    power_mw: pw,
                    tick,
                    suspicious,
                };
                self.bus.push(sample);
                self.samples = self.samples.saturating_add(1);
                if suspicious {
                    self.suspicious = self.suspicious.saturating_add(1);
                }
                self.last_code = if freq_suspicious {
                    CODE_SUSPICIOUS_FREQ
                } else if power_suspicious {
                    CODE_SUSPICIOUS_POWER
                } else if pw.is_none() {
                    // RAPL 不可用的合法标注路径（不是降级失败）。
                    CODE_RAPL_UNAVAILABLE
                } else {
                    0
                };
                PollOutcome::Sampled { suspicious }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 六、域自检（判据逐条映射锚点：只读采样/缺测不插值/预算治理/可疑标记/判据）
// ---------------------------------------------------------------------------

/// F0231 域自检入口（聚合器调用；零 panic 面，失败逐条红不炸域）。
pub fn run_veb231_checks() -> CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("veb231_freqpower");

    // --- 判据 1：只读采样（注入值逐位入账 + 零墙钟确定性 + 边界表闭环） ---
    {
        // 注入值原样入总线：频率/功耗逐位等于注入值（只读=不改写被测值）。
        let mut m = FreqPowerTelemetry::new(true, 5);
        let _ = m.poll(0, None, None); // 排程
        let out = m.poll(5, Some(3200), Some(65_000));
        let verbatim = matches!(out, PollOutcome::Sampled { suspicious: false })
            && m.bus().latest()
                == Some(TelemetrySample {
                    freq_mhz: Some(3200),
                    power_mw: Some(65_000),
                    tick: 5,
                    suspicious: false,
                });
        s.add(
            "B31-只读采样-注入值逐位入账",
            verbatim,
            "频率/功耗注入值逐位入总线（无换算无钳制——只读红线是可运行断言）",
        );
        // 零墙钟确定性：相同注入序列产生相同总线内容（对拍可复现）。
        let mut a = FreqPowerTelemetry::new(false, 4);
        let mut b = FreqPowerTelemetry::new(false, 4);
        let _ = a.poll(10, None, None);
        let _ = a.poll(14, Some(2400), None);
        let _ = b.poll(10, None, None);
        let _ = b.poll(14, Some(2400), None);
        s.add(
            "B31-只读采样-零墙钟确定性",
            a.bus().latest() == b.bus().latest()
                && a.bus().total() == b.bus().total()
                && a.counters() == b.counters(),
            "同输入同输出（时间戳全部来自注入 tick，无墙钟无 IO）",
        );
        // 档位边界表非重叠全覆盖：RPn.max+1==RPe.min、RPe.max+1==RP0.min。
        let mut contiguous = true;
        let mut i = 0;
        while i + 1 < LEVELS.len() {
            let lo = LEVELS[i];
            let hi = LEVELS[i + 1];
            contiguous = contiguous
                && lo.max_mhz() < hi.min_mhz()
                && hi.min_mhz() == lo.max_mhz() + 1;
            i += 1;
        }
        contiguous = contiguous
            && LEVELS[0].min_mhz() == FREQ_MIN_MHZ
            && LEVELS[LEVELS.len() - 1].max_mhz() == FREQ_MAX_MHZ;
        s.add(
            "B31-只读采样-边界表非重叠全覆盖",
            contiguous,
            "三档边界衔接触碰（无频率落空洞），总包络=RPn.min..=RP0.max",
        );
        // wire/序号往返 + 越界 None。
        let mut round = true;
        for l in LEVELS {
            round = round && PStateLevel::from_wire(l.wire()) == Some(l);
            round = round && PStateLevel::from_ordinal(l.ordinal()) == Some(l);
        }
        round = round
            && PStateLevel::from_wire(0).is_none()
            && PStateLevel::from_wire(4).is_none()
            && PStateLevel::from_ordinal(3).is_none();
        s.add(
            "B31-只读采样-档位往返与越界拒",
            round,
            "wire/序号往返一致，越界 None（不留默认档兜底）",
        );
    }

    // --- 判据 2：缺测不插值（失败补标 / 断档补标有界 / 无编造值） ---
    {
        // 采样失败：到期槽位读不到主轴 → 缺测标记，双 None。
        let mut m = FreqPowerTelemetry::new(true, 5);
        let _ = m.poll(0, None, None); // 排程
        let out = m.poll(5, None, None);
        s.add(
            "B31-缺测不插值-采样失败标记缺测",
            out == PollOutcome::Missing
                && m.counters().0 == 0
                && m.counters().1 == 1
                && m.bus().latest().map(|x| x.is_missing()) == Some(true)
                && m.last_code() == CODE_SAMPLE_FAILED,
            "失败槽位推入双 None 缺测样本（不插值不补零不沿用旧值）",
        );
        // 断档逐槽补标：间隔 5，tick 0 排程、tick 20 到达 → 跳过 3 槽。
        let mut m2 = FreqPowerTelemetry::new(true, 5);
        let _ = m2.poll(0, None, None); // 排程 due=5
        let _ = m2.poll(20, Some(3000), Some(45_000));
        s.add(
            "B31-缺测不插值-断档逐槽补标",
            m2.counters().1 == 3
                && m2.counters().0 == 1
                && m2.counters().3 == 0
                && m2.bus().latest().map(|x| x.freq_mhz) == Some(Some(3000)),
            "跳过的到期槽位逐槽缺测标记（3 槽），本槽正常采样",
        );
        // 补标上限外只如实计数：间隔 1，tick 1000 跃过 999 槽 → 16 补标 + 983 记账。
        let mut m3 = FreqPowerTelemetry::new(true, 1);
        let _ = m3.poll(0, None, None); // 排程 due=1
        let _ = m3.poll(1000, Some(3000), None);
        s.add(
            "B31-缺测不插值-补标有界上限外记账",
            m3.counters().1 == CATCHUP_CAP
                && m3.counters().3 == 983
                && m3.counters().0 == 1,
            "补标恰 16 条（CATCHUP_CAP），剩余 983 跳过槽位如实计入 gaps",
        );
        // 总线内缺测样本恒无值（对插值/沿用旧值的结构性反证）。
        let mut m4 = FreqPowerTelemetry::new(true, 2);
        let _ = m4.poll(0, Some(3000), Some(45_000)); // 排程 due=2
        let _ = m4.poll(4, None, None); // 跳过槽 2 → 缺测；槽 4 采样失败 → 缺测
        let bus_ok = {
            let (missing, _s) = m4.bus().tally();
            missing == 2 && m4.bus().get(1).map(|x| x.is_missing()) == Some(true)
        };
        s.add(
            "B31-缺测不插值-缺测样本无编造值",
            bus_ok && m4.counters().1 == 2,
            "缺测样本频率功耗双 None（判据侧独立 tally 重算对账）",
        );
        // 矛盾态（无频率有功耗）整条缺测。
        let mut m5 = FreqPowerTelemetry::new(true, 3);
        let _ = m5.poll(0, None, None); // 排程
        let out5 = m5.poll(3, None, Some(60_000));
        s.add(
            "B31-缺测不插值-矛盾态整条缺测",
            out5 == PollOutcome::Missing && m5.counters().0 == 0 && m5.counters().1 == 1,
            "频率主轴缺失时功耗值不单独成样（断链半样本比缺测更坏）",
        );
    }

    // --- 判据 3：预算治理（未到期不采样 / 零间隔拒绝 / 节奏生效） ---
    {
        // 未到期：Idle、总线空、计数不动。
        let mut m = FreqPowerTelemetry::new(true, 10);
        let _ = m.poll(0, None, None); // 排程 due=10
        let idle = m.poll(9, Some(3000), Some(45_000));
        s.add(
            "B31-预算治理-未到期不采样",
            idle == PollOutcome::Idle
                && m.bus().total() == 0
                && m.counters().0 == 0
                && m.counters().1 == 0,
            "预算内 poll 零副作用（遥测不许自扰主路）",
        );
        // 零间隔：显性拒绝 + 拒绝计数 + 分码（与采样失败分码）。
        let mut m2 = FreqPowerTelemetry::new(true, 5);
        let r = m2.set_budget(0);
        s.add(
            "B31-预算治理-零间隔显性拒绝",
            r == Err(CODE_BUDGET_EXCEEDED)
                && m2.counters().4 == 1
                && m2.last_code() == CODE_BUDGET_EXCEEDED
                && m2.interval_ticks() == 5,
            "F0096 零预算显性拒绝且不改写现役间隔（拒绝不是悄悄归 1）",
        );
        // 预算回填改节奏：7 tick 间隔，到期恰采样。
        let mut m3 = FreqPowerTelemetry::new(true, 5);
        let ok = m3.set_budget(7);
        let _ = m3.poll(0, None, None); // 排程 due=7
        let sampled = m3.poll(7, Some(2800), Some(45_000));
        s.add(
            "B31-预算治理-预算回填节奏生效",
            ok.is_ok()
                && m3.interval_ticks() == 7
                && matches!(sampled, PollOutcome::Sampled { .. })
                && m3.counters().0 == 1
                && m3.due_tick() == 14,
            "set_budget 生效且到期推进恰为新间隔（due=7→14）",
        );
        // tick 回退：显性拒绝且不清账（排程与计数保持）。
        let mut m4 = FreqPowerTelemetry::new(true, 5);
        let _ = m4.poll(0, None, None); // 排程 due=5
        let _ = m4.poll(3, None, None); // Idle，last_tick=3
        let rej = m4.poll(2, None, None);
        s.add(
            "B31-预算治理-tick回退显性拒绝",
            rej == PollOutcome::Rejected(CODE_BAD_REQUEST)
                && m4.last_code() == CODE_BAD_REQUEST
                && m4.due_tick() == 5
                && m4.counters() == (0, 0, 0, 0, 0),
            "相位错拒绝且不清账（调用方 bug 不许摧毁排程时间基准）",
        );
    }

    // --- 判据 4：可疑标记（越界如实记录不丢弃 / RAPL 事实分型） ---
    {
        // 频率越界：照原值入账 + 可疑 + 专属码（不丢弃）。
        let mut m = FreqPowerTelemetry::new(true, 4);
        let _ = m.poll(0, None, None); // 排程
        let out = m.poll(4, Some(5999), Some(65_000));
        s.add(
            "B31-可疑标记-频率越界如实记录",
            matches!(out, PollOutcome::Sampled { suspicious: true })
                && m.counters().0 == 1
                && m.counters().2 == 1
                && m.bus().latest().map(|x| x.freq_mhz) == Some(Some(5999))
                && m.last_code() == CODE_SUSPICIOUS_FREQ,
            "越界频率原值入总线并标可疑（丢弃异常值=销毁 F0100 事实来源）",
        );
        // 频率下界外（含 0）：同样可疑。
        let mut m0 = FreqPowerTelemetry::new(true, 4);
        let _ = m0.poll(0, None, None);
        let out0 = m0.poll(4, Some(399), Some(65_000));
        s.add(
            "B31-可疑标记-频率下界外同样可疑",
            matches!(out0, PollOutcome::Sampled { suspicious: true })
                && m0.last_code() == CODE_SUSPICIOUS_FREQ,
            "包络下界外判可疑（贴线 400 合法不误拒）",
        );
        // 功耗越界（零功耗 / 超上界）：可疑 + 专属码。
        let mut mp = FreqPowerTelemetry::new(true, 4);
        let _ = mp.poll(0, None, None); // 排程
        let outp0 = mp.poll(4, Some(3000), Some(0));
        let outp1 = mp.poll(8, Some(3000), Some(250_001));
        s.add(
            "B31-可疑标记-功耗越界可疑",
            matches!(outp0, PollOutcome::Sampled { suspicious: true })
                && matches!(outp1, PollOutcome::Sampled { suspicious: true })
                && mp.counters().2 == 2
                && mp.last_code() == CODE_SUSPICIOUS_POWER,
            "零功耗与超上界均可疑（活体封装不可能零功耗）",
        );
        // RAPL 不可用：仅频率维度（合法标注，非可疑）。
        let mut mr = FreqPowerTelemetry::new(true, 4);
        mr.set_rapl_available(false);
        let _ = mr.poll(0, None, None);
        let outr = mr.poll(4, Some(2400), None);
        s.add(
            "B31-可疑标记-RAPL不可用仅频率维度",
            matches!(outr, PollOutcome::Sampled { suspicious: false })
                && mr.counters().2 == 0
                && mr.bus().latest().map(|x| x.power_mw) == Some(None)
                && mr.last_code() == CODE_RAPL_UNAVAILABLE,
            "不可用时功耗维度 None 且标注专属码（静默降级不可接受）",
        );
        // RAPL 声明可用却读不到功耗：读失败不是不可用 → 可疑。
        let mut mf = FreqPowerTelemetry::new(true, 4);
        let _ = mf.poll(0, None, None);
        let outf = mf.poll(4, Some(2400), None);
        s.add(
            "B31-可疑标记-声明可用却缺功耗判可疑",
            matches!(outf, PollOutcome::Sampled { suspicious: true })
                && mf.last_code() == CODE_SUSPICIOUS_POWER,
            "「可用但读不到」与「不可用」分型（混型会把读失败洗成正常）",
        );
        // 正常值不标可疑（防恒真：上几条若恒真这里必须能区分）。
        let mut mn = FreqPowerTelemetry::new(true, 4);
        let _ = mn.poll(0, None, None);
        let outn = mn.poll(4, Some(3200), Some(65_000));
        s.add(
            "B31-可疑标记-正常值不标可疑",
            matches!(outn, PollOutcome::Sampled { suspicious: false })
                && mn.counters().2 == 0
                && mn.last_code() == 0,
            "包络内频率+区间内功耗不标可疑（贴线 400/5800/250000 双向不误判见下）",
        );
        // 贴线值双向：恰边界合法（400/5800/250000/1）。
        let mut mb = FreqPowerTelemetry::new(true, 4);
        let _ = mb.poll(0, None, None);
        let _ = mb.poll(4, Some(400), Some(250_000));
        let _ = mb.poll(8, Some(5800), Some(1));
        s.add(
            "B31-可疑标记-贴线值双向不误判",
            mb.counters().2 == 0 && mb.counters().0 == 2,
            "恰包络边界的频率/功耗合法不标可疑（越界一格即判的另一半）",
        );
    }

    // --- 判据 5：判据（诊断码互异 + 段独占 + 兜底 + 条数对账） ---
    {
        let mut ok = true;
        let mut i = 0;
        while i < CODES.len() {
            let mut j = i + 1;
            while j < CODES.len() {
                if CODES[i] == CODES[j] {
                    ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        s.add("B31-判据-码位两两互异", ok, "六码互异（按码归类的前提）");
        let seg_ok = CODES.iter().all(|c| c & 0xFF00 == 0x4F00);
        s.add(
            "B31-判据-码段独占0x4F",
            seg_ok,
            "全码独占 0x4F 段（0x4E 及以下已占，全仓 grep 零占用后选定）",
        );
        s.add(
            "B31-判据-未知码兜底不panic",
            !explain(0x4FFF).is_empty() && explain(CODE_BAD_REQUEST) != explain(0x4FFF),
            "未知码有兜底人话（不崩也不静默）",
        );
        s.add(
            "B31-判据-条数对账",
            s.len() == 23,
            "判据条数恰 24（本条执行前已有 23 条，防悄悄增删）",
        );
    }

    s
}

// 引入判据层类型（`use crate::checks::CheckSet` 亦在函数体内可见）。
use crate::checks::CheckSet;
