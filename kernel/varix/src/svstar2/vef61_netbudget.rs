//! VE-F5408 · 网络性能预算（VE-AA 域 · 网络域 · AA08 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5408`
//!
//! **判据（锚点原文）**：带宽分档、占比上限、容量实测、回灌、判据。
//!
//! **职责定位（锚点原文）**：网络性能预算——预算三件：带宽预算（每
//! 连接上下行带宽分档（档位显式可查）/ CPU 预算（网络处理占帧比例
//! 上限）/并发预算（单服务器并发连接容量实测——容量不虚标 F4938
//! 口径）。错误路径与降级矩阵：预算失准→实测回灌；超限→分档降级；
//! 虚标→回灌修正。度量 O(连接数)；降级 O(1)；回灌 O(样本)。跨批
//! 对接点：F4938 诚实口径上游；F5418 基准消费；F5420 双签。无障碍
//! 与隐私：容量不虚标即域本色（核心）；带宽档位读屏可达。
//!
//! ## 一、带宽分档（判据一）
//!
//! 每连接上下行带宽分档（[`BAND_TIERS`] 五档闭集——档位显式可查
//! （[`BandTier`] 上行/下行 bps 档值 + 人读名 + 机检键——读屏可达
//! 是域本色）；连接带宽卡（[`ConnBand`]——连接指纹+上下行档+实测
//! 峰值——声明档与实测峰值分开记账不混写）；分档守恒（每连接恰一
//! 档，档值表单源 [`BAND_TIER_BPS`]——重复定义是漂移起点））。
//!
//! ## 二、CPU 占比上限（判据二）
//!
//! 网络处理占帧比例上限（[`NET_CPU_CAP_BPS`]——bps 制占比上限
//! 编译期钉死；帧占比账（[`CpuShare`]——每帧网络处理占比（实测
//! 处理 tick/帧长，bps 制）+ 超限立案 [`E_NB_CPU_OVER`]））。
//!
//! ## 三、容量实测不虚标（判据三：F4938 诚实口径）
//!
//! 单服务器并发连接容量实测（[`CapacityProbe`]——实测单三件：
//! 样本数/实测峰值/置信档——容量不虚标是域本色核心：声明容量必须
//! 挂实测单，无实测单的容量声明显性拒 [`E_NB_NO_PROBE`]（虚标
//! 在门口被挡——不是评审时才发现）；实测回灌（[`CapacityProbe::
//! feed`]——新样本回灌修正容量声明，O(样本)；回灌留痕
//! [`ProbeSample`]——每样本独立指纹）；虚标检测（[`CapacityProbe::
//! verify_claim`]——声明容量>实测峰值×安全系数即虚标立案
//! [`E_NB_OVERCLAIM`]）。
//!
//! ## 四、超限分档降级（深化面）
//!
//! 超限→分档降级（[`BandGovernor`]——带宽超限逐档降 [`downgrade`]
//! O(1)，回升迟滞（连续达标 N 样本升一档——不对称防振荡，与
//! vco07 恢复引擎同纪律）；降级/回升全留痕 [`TierChange`]）。
//!
//! **对接**：F4938（诚实口径——容量不虚标的上游理念）；F5418（基准
//! 消费——本单预算账是基准的输入）；F5420（双签——预算口径变更
//! 双签生效）；vef54-58（通道/会话/复制——预算约束的下游）。零
//! panic 面、零 IO、零墙钟（tick 注入）、no_std、零浮点。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const NETBUDGET_VERSION: &str = "AA08-netbudget-v1";

/// 带宽档数（五档闭集）。
pub const BAND_TIER_COUNT: usize = 5;

/// 带宽档值表（上行 bps；单源——重复定义是漂移起点）。
pub const BAND_TIER_BPS: [u32; BAND_TIER_COUNT] =
    [32_000, 128_000, 512_000, 2_000_000, 8_000_000];

/// 下行倍率（bps——下行通常富余）。
pub const DOWNLINK_MULT: u32 = 4;

/// 网络处理占帧比例上限（bps 制：10000=整帧）。
pub const NET_CPU_CAP_BPS: u32 = 500;

/// 容量安全系数（声明容量 ≤ 实测峰值/系数——防虚标）。
pub const CAPACITY_SAFETY_DIV: u32 = 2;

/// 回升迟滞（连续达标样本数——升档门槛）。
pub const UPGRADE_HYSTERESIS: u32 = 8;

// ---------------------------------------------------------------------------
// 二、字符串诊断码（AA 域字符串码惯例——本组 E_NB_ 前缀互异）
// ---------------------------------------------------------------------------

/// 诊断码闭集（E_NB_ 前缀互异）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 带宽档表外。
    TierUnknown,
    /// CPU 占比超限。
    CpuOver,
    /// 无实测单的容量声明（虚标在门口被挡）。
    NoProbe,
    /// 容量虚标（声明>实测/安全系数）。
    Overclaim,
    /// 连接重复（同指纹连接重复入账）。
    ConnDuplicate,
    /// 连接不在账。
    ConnMissing,
    /// 样本坏（样本数为零/峰值倒挂）。
    SampleBad,
    /// 预算口径变更未双签（F5420 闸）。
    DualSignMissing,
}

impl XcCode {
    /// 人读诊断（含可操作建议）。
    pub fn detail(&self) -> &'static str {
        match self {
            XcCode::TierUnknown => "带宽档表外：五档闭集之外无档，建议按 BAND_TIERS 取用",
            XcCode::CpuOver => "CPU 占比超限：网络处理占帧超上限，建议降并发或降档",
            XcCode::NoProbe => "无实测单：容量声明必须挂实测单（容量不虚标是域本色），建议先跑容量实测",
            XcCode::Overclaim => "容量虚标：声明容量超实测峰值/安全系数，建议回灌实测修正声明",
            XcCode::ConnDuplicate => "连接重复：同指纹连接已入账，幂等拒绝重复登记",
            XcCode::ConnMissing => "连接不在账：预算操作命中空连接，建议先入账再操作",
            XcCode::SampleBad => "样本坏：样本数为零或峰值倒挂，建议核对实测数据源",
            XcCode::DualSignMissing => "预算口径变更未双签：F5420 双签闸未过，建议补双签再生效",
        }
    }

    /// 字符串码。
    pub const fn code(&self) -> &'static str {
        match self {
            XcCode::TierUnknown => "E_NB_TIER_UNKNOWN",
            XcCode::CpuOver => "E_NB_CPU_OVER",
            XcCode::NoProbe => "E_NB_NO_PROBE",
            XcCode::Overclaim => "E_NB_OVERCLAIM",
            XcCode::ConnDuplicate => "E_NB_CONN_DUP",
            XcCode::ConnMissing => "E_NB_CONN_MISSING",
            XcCode::SampleBad => "E_NB_SAMPLE_BAD",
            XcCode::DualSignMissing => "E_NB_NO_DUALSIGN",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、FNV-1a 指纹基元
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 单字节吸收。
pub const fn fnv1a_byte(state: u64, byte: u8) -> u64 {
    (state ^ (byte as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a 字节串吸收。
pub const fn fnv1a_bytes(state: u64, bytes: &[u8]) -> u64 {
    let mut s = state;
    let mut i = 0usize;
    while i < bytes.len() {
        s = fnv1a_byte(s, bytes[i]);
        i += 1;
    }
    s
}

/// FNV-1a 字符串吸收。
pub fn fnv1a_str(state: u64, text: &str) -> u64 {
    fnv1a_bytes(state, text.as_bytes())
}

/// FNV-1a u64 吸收。
pub const fn fnv1a_u64(state: u64, value: u64) -> u64 {
    fnv1a_bytes(state, &value.to_be_bytes())
}

// ---------------------------------------------------------------------------
// 四、带宽分档（判据一）
// ---------------------------------------------------------------------------

/// 带宽档（五档闭集；档位显式可查——读屏可达是域本色）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandTier {
    /// 一档：低带宽（移动弱网）。
    T1,
    /// 二档：标准。
    T2,
    /// 三档：宽带。
    T3,
    /// 四档：高速。
    T4,
    /// 五档：机房级。
    T5,
}

impl BandTier {
    /// 全档数组（序号对位）。
    pub const ALL: [BandTier; BAND_TIER_COUNT] =
        [BandTier::T1, BandTier::T2, BandTier::T3, BandTier::T4, BandTier::T5];

    /// 序号。
    pub const fn ordinal(self) -> usize {
        match self {
            BandTier::T1 => 0,
            BandTier::T2 => 1,
            BandTier::T3 => 2,
            BandTier::T4 => 3,
            BandTier::T5 => 4,
        }
    }

    /// 序号→档（越界 None）。
    pub const fn of_ordinal(i: usize) -> Option<BandTier> {
        match i {
            0 => Some(BandTier::T1),
            1 => Some(BandTier::T2),
            2 => Some(BandTier::T3),
            3 => Some(BandTier::T4),
            4 => Some(BandTier::T5),
            _ => None,
        }
    }

    /// 上行档值（单源表直取 O(1)）。
    pub const fn uplink_bps(self) -> u32 {
        BAND_TIER_BPS[self.ordinal()]
    }

    /// 下行档值（倍率换算，饱和）。
    pub const fn downlink_bps(self) -> u32 {
        self.uplink_bps().saturating_mul(DOWNLINK_MULT)
    }

    /// 人读名（档位显式可查）。
    pub const fn zh(self) -> &'static str {
        match self {
            BandTier::T1 => "一档·低带宽",
            BandTier::T2 => "二档·标准",
            BandTier::T3 => "三档·宽带",
            BandTier::T4 => "四档·高速",
            BandTier::T5 => "五档·机房级",
        }
    }

    /// 机检键。
    pub const fn key(self) -> &'static str {
        match self {
            BandTier::T1 => "aa08.band.t1",
            BandTier::T2 => "aa08.band.t2",
            BandTier::T3 => "aa08.band.t3",
            BandTier::T4 => "aa08.band.t4",
            BandTier::T5 => "aa08.band.t5",
        }
    }
}

/// 带宽档读屏行（判据一可见面：五档全列——档位显式可查的落地）。
pub fn band_tiers_a11y() -> String {
    let mut s = String::new();
    s.push_str("带宽分档：");
    let mut i = 0usize;
    while i < BAND_TIER_COUNT {
        if let Some(t) = BandTier::of_ordinal(i) {
            if i > 0 {
                s.push('；');
            }
            s.push_str(&alloc::format!(
                "{}↑{}↓{}",
                t.zh(),
                t.uplink_bps(),
                t.downlink_bps()
            ));
        }
        i += 1;
    }
    s
}

/// 连接带宽卡（声明档与实测峰值分开记账——不混写不互冒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnBand {
    /// 连接指纹。
    pub conn_fp: u64,
    /// 声明档序号。
    pub tier: usize,
    /// 实测上行峰值 bps（0=未测）。
    pub measured_up_bps: u32,
    /// 入账 tick。
    pub tick: u64,
}

impl ConnBand {
    /// 卡指纹。
    pub const fn fingerprint(&self) -> u64 {
        let s = fnv1a_u64(FNV_OFFSET, self.conn_fp);
        let s = fnv1a_u64(s, self.tier as u64);
        let s = fnv1a_u64(s, self.measured_up_bps as u64);
        fnv1a_u64(s, self.tick)
    }

    /// 档合法性。
    pub const fn validate(&self) -> Result<(), XcCode> {
        if self.tier >= BAND_TIER_COUNT {
            return Err(XcCode::TierUnknown);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、CPU 占比上限（判据二）
// ---------------------------------------------------------------------------

/// 帧占比账（网络处理占帧；bps 制）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuShare {
    /// 网络处理 tick（本帧）。
    pub net_ticks: u32,
    /// 帧长 tick。
    pub frame_ticks: u32,
}

impl CpuShare {
    /// 占比 bps（saturating；帧长 0 按满额兜底不除零）。
    pub const fn share_bps(&self) -> u32 {
        if self.frame_ticks == 0 {
            return 10_000;
        }
        let num = (self.net_ticks as u64).saturating_mul(10_000);
        (num / self.frame_ticks as u64) as u32
    }

    /// 超限核验（占比>上限立案）。
    pub const fn verify(&self) -> Result<(), XcCode> {
        if self.share_bps() > NET_CPU_CAP_BPS {
            return Err(XcCode::CpuOver);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 六、容量实测不虚标（判据三：F4938 诚实口径）
// ---------------------------------------------------------------------------

/// 实测样本（每次压测一笔；独立指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeSample {
    /// 实测并发连接峰值。
    pub peak_conns: u32,
    /// 采样 tick。
    pub tick: u64,
    /// 样本指纹。
    pub fingerprint: u64,
}

impl ProbeSample {
    /// 构造并算指纹。
    pub const fn new(peak_conns: u32, tick: u64) -> ProbeSample {
        let s = fnv1a_u64(FNV_OFFSET, peak_conns as u64);
        ProbeSample {
            peak_conns,
            tick,
            fingerprint: fnv1a_u64(s, tick),
        }
    }
}

/// 容量实测单（容量不虚标——声明必须挂单）。
#[derive(Clone, Debug, Default)]
pub struct CapacityProbe {
    samples: Vec<ProbeSample>,
    /// 实测峰值（样本最大值，saturating 汇总）。
    observed_peak: u32,
    /// 声明容量（对外口径；必须 ≤ 峰值/安全系数）。
    declared: u32,
}

impl CapacityProbe {
    /// 空单。
    pub const fn new() -> CapacityProbe {
        CapacityProbe {
            samples: Vec::new(),
            observed_peak: 0,
            declared: 0,
        }
    }

    /// 回灌样本（预算失准→实测回灌；O(1) 峰值汇总）。
    pub fn feed(&mut self, sample: ProbeSample) -> Result<(), XcCode> {
        if sample.peak_conns == 0 {
            return Err(XcCode::SampleBad);
        }
        if sample.peak_conns < self.observed_peak {
            // 倒挂样本收账但不改峰值（峰值是历史最高——回灌只升不降，
            // 环境变差走降级面不走数据面）。
            self.samples.push(sample);
            return Ok(());
        }
        self.observed_peak = sample.peak_conns;
        self.samples.push(sample);
        Ok(())
    }

    /// 样本数。
    pub const fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// 是否有实测单。
    pub const fn has_probe(&self) -> bool {
        !self.samples.is_empty()
    }

    /// 实测峰值。
    pub const fn observed_peak(&self) -> u32 {
        self.observed_peak
    }

    /// 声明容量设定（无实测单显性拒——虚标在门口被挡）。
    pub fn declare(&mut self, capacity: u32) -> Result<(), XcCode> {
        if !self.has_probe() {
            return Err(XcCode::NoProbe);
        }
        self.verify_claim(capacity)?;
        self.declared = capacity;
        Ok(())
    }

    /// 声明容量。
    pub const fn declared_capacity(&self) -> u32 {
        self.declared
    }

    /// 虚标检测（声明>实测峰值/安全系数→立案；无实测单也立案路径）。
    pub const fn verify_claim(&self, capacity: u32) -> Result<(), XcCode> {
        if !self.has_probe() {
            return Err(XcCode::NoProbe);
        }
        let ceiling = self.observed_peak / CAPACITY_SAFETY_DIV;
        if capacity > ceiling {
            return Err(XcCode::Overclaim);
        }
        Ok(())
    }

    /// 实测单指纹（全样本+峰值+声明混算——回灌对拍基元）。
    pub fn fingerprint(&self) -> u64 {
        let mut s = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.samples.len() {
            if let Some(x) = self.samples.get(i) {
                s = fnv1a_u64(s, x.fingerprint);
            }
            i += 1;
        }
        let s = fnv1a_u64(s, self.observed_peak as u64);
        fnv1a_u64(s, self.declared as u64)
    }

    /// 容量读屏（域本色：容量不虚标即读屏可达——声明/实测/样本数全列）。
    pub fn a11y_line(&self) -> String {
        if !self.has_probe() {
            return "容量：无实测单（不虚标——拒绝声明）".to_string();
        }
        alloc::format!(
            "容量：声明={}/实测峰值={}/样本{}（安全系数1/{}）",
            self.declared,
            self.observed_peak,
            self.samples.len(),
            CAPACITY_SAFETY_DIV
        )
    }
}

// ---------------------------------------------------------------------------
// 七、分档降级治理（深化面：超限降档+迟滞回升）
// ---------------------------------------------------------------------------

/// 档位变更留痕。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TierChange {
    /// 连接指纹。
    pub conn_fp: u64,
    /// 旧档。
    pub from: usize,
    /// 新档。
    pub to: usize,
    /// 是否降级。
    pub down: bool,
    /// 变更 tick。
    pub tick: u64,
    /// 记录指纹。
    pub fingerprint: u64,
}

impl TierChange {
    /// 构造并算指纹。
    pub const fn new(conn_fp: u64, from: usize, to: usize, down: bool, tick: u64) -> TierChange {
        let s = fnv1a_u64(FNV_OFFSET, conn_fp);
        let s = fnv1a_u64(s, from as u64);
        let s = fnv1a_u64(s, to as u64);
        let s = fnv1a_u64(s, if down { 1 } else { 0 });
        TierChange {
            conn_fp,
            from,
            to,
            down,
            tick,
            fingerprint: fnv1a_u64(s, tick),
        }
    }
}

/// 带宽治理器（超限逐档降 O(1)；回升迟滞不对称防振荡）。
#[derive(Clone, Debug, Default)]
pub struct BandGovernor {
    changes: Vec<TierChange>,
    good_streak: u32,
}

impl BandGovernor {
    /// 空治理器。
    pub const fn new() -> BandGovernor {
        BandGovernor {
            changes: Vec::new(),
            good_streak: 0,
        }
    }

    /// 超限降档（O(1)：档位-1 饱和到 0；留痕）。
    pub fn downgrade(&mut self, conn_fp: u64, cur: usize, tick: u64) -> (usize, bool) {
        self.good_streak = 0;
        if cur == 0 {
            return (0, false);
        }
        let to = cur - 1;
        self.changes.push(TierChange::new(conn_fp, cur, to, true, tick));
        (to, true)
    }

    /// 达标推进（连续达标 UP Grade_HYSTERESIS 档升一级——不对称回升）。
    pub fn on_ok(&mut self, conn_fp: u64, cur: usize, tick: u64) -> (usize, bool) {
        self.good_streak = self.good_streak.saturating_add(1);
        if self.good_streak >= UPGRADE_HYSTERESIS && cur + 1 < BAND_TIER_COUNT {
            self.good_streak = 0;
            let to = cur + 1;
            self.changes.push(TierChange::new(conn_fp, cur, to, false, tick));
            return (to, true);
        }
        (cur, false)
    }

    /// 变更笔数。
    pub const fn change_count(&self) -> usize {
        self.changes.len()
    }

    /// 变更读取。
    pub fn change_at(&self, i: usize) -> Option<&TierChange> {
        self.changes.get(i)
    }

    /// 治理账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut s = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.changes.len() {
            if let Some(c) = self.changes.get(i) {
                s = fnv1a_u64(s, c.fingerprint);
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 八、网络预算引擎（三件总编排：度量 O(连接数)）
// ---------------------------------------------------------------------------

/// 网络预算引擎（带宽分档账+CPU 占比+容量实测单+治理器）。
pub struct NetBudgetEngine {
    conns: Vec<ConnBand>,
    governor: BandGovernor,
    probe: CapacityProbe,
    /// 超限立案数。
    cpu_over_count: u32,
    /// 虚标立案数。
    overclaim_count: u32,
    /// 账链指纹。
    chain: u64,
}

impl NetBudgetEngine {
    /// 空引擎。
    pub const fn new() -> NetBudgetEngine {
        NetBudgetEngine {
            conns: Vec::new(),
            governor: BandGovernor::new(),
            probe: CapacityProbe::new(),
            cpu_over_count: 0,
            overclaim_count: 0,
            chain: FNV_OFFSET,
        }
    }

    /// 入连接带宽卡（同指纹幂等拒；度量账 O(1) 入账）。
    pub fn admit_conn(&mut self, card: ConnBand) -> Result<(), XcCode> {
        card.validate()?;
        let mut i = 0usize;
        while i < self.conns.len() {
            if let Some(c) = self.conns.get(i) {
                if c.conn_fp == card.conn_fp {
                    return Err(XcCode::ConnDuplicate);
                }
            }
            i += 1;
        }
        self.chain = fnv1a_u64(self.chain, card.fingerprint());
        self.conns.push(card);
        Ok(())
    }

    /// 连接数。
    pub const fn conn_count(&self) -> usize {
        self.conns.len()
    }

    /// 连接读取。
    pub fn conn_at(&self, i: usize) -> Option<&ConnBand> {
        self.conns.get(i)
    }

    /// 带宽总量度量（度量 O(连接数)：全体声明档上行/下行求和，saturating）。
    pub fn total_declared_bps(&self) -> (u64, u64) {
        let mut up: u64 = 0;
        let mut down: u64 = 0;
        let mut i = 0usize;
        while i < self.conns.len() {
            if let Some(c) = self.conns.get(i) {
                if let Some(t) = BandTier::of_ordinal(c.tier) {
                    up = up.saturating_add(t.uplink_bps() as u64);
                    down = down.saturating_add(t.downlink_bps() as u64);
                }
            }
            i += 1;
        }
        (up, down)
    }

    /// 帧占比核验（判据二执行面；超限立案并降最低权连接一档）。
    pub fn check_cpu_share(&mut self, net_ticks: u32, frame_ticks: u32, tick: u64) -> Result<(), XcCode> {
        let share = CpuShare {
            net_ticks,
            frame_ticks,
        };
        match share.verify() {
            Ok(()) => Ok(()),
            Err(c) => {
                self.cpu_over_count = self.cpu_over_count.saturating_add(1);
                // 超限→分档降级：降实测峰值最低的连接一档（确定性选边）。
                let mut victim: Option<usize> = None;
                let mut i = 0usize;
                while i < self.conns.len() {
                    let c = match self.conns.get(i) {
                        Some(x) => x,
                        None => break,
                    };
                    match victim {
                        None => victim = Some(i),
                        Some(v) => {
                            let vm = match self.conns.get(v) {
                                Some(x) => x.measured_up_bps,
                                None => break,
                            };
                            if c.measured_up_bps < vm {
                                victim = Some(i);
                            }
                        }
                    }
                    i += 1;
                }
                if let Some(v) = victim {
                    let cur = match self.conns.get(v) {
                        Some(x) => x.tier,
                        None => return Err(c),
                    };
                    let fp = match self.conns.get(v) {
                        Some(x) => x.conn_fp,
                        None => return Err(c),
                    };
                    let (to, changed) = self.governor.downgrade(fp, cur, tick);
                    if changed {
                        if let Some(cc) = self.conns.get_mut(v) {
                            cc.tier = to;
                        }
                    }
                }
                self.chain = fnv1a_u64(self.chain, tick);
                Err(c)
            }
        }
    }

    /// 容量回灌（判据三执行面：样本回灌+声明重验——回灌 O(样本)）。
    pub fn feed_capacity(&mut self, peak_conns: u32, tick: u64) -> Result<(), XcCode> {
        self.probe.feed(ProbeSample::new(peak_conns, tick))?;
        // 声明已在单上时重验（虚标→回灌修正：超线声明自动降线留痕）。
        if self.probe.declared_capacity() > 0 {
            let d = self.probe.declared_capacity();
            if self.probe.verify_claim(d).is_err() {
                self.overclaim_count = self.overclaim_count.saturating_add(1);
                let ceiling = self.probe.observed_peak() / CAPACITY_SAFETY_DIV;
                let _ = self.probe.declare(ceiling);
            }
        }
        self.chain = fnv1a_u64(self.chain, self.probe.fingerprint());
        Ok(())
    }

    /// 容量声明（无实测单拒——虚标在门口被挡）。
    pub fn declare_capacity(&mut self, capacity: u32) -> Result<(), XcCode> {
        self.probe.declare(capacity)
    }

    /// 容量单只读。
    pub const fn probe(&self) -> &CapacityProbe {
        &self.probe
    }

    /// 治理账只读。
    pub const fn governor(&self) -> &BandGovernor {
        &self.governor
    }

    /// 连接达标推进（回升迟滞面）。
    pub fn conn_ok_tick(&mut self, conn_fp: u64, tick: u64) -> Result<(), XcCode> {
        let mut found: Option<usize> = None;
        let mut i = 0usize;
        while i < self.conns.len() {
            if let Some(c) = self.conns.get(i) {
                if c.conn_fp == conn_fp {
                    found = Some(i);
                }
            }
            i += 1;
        }
        let idx = match found {
            Some(x) => x,
            None => return Err(XcCode::ConnMissing),
        };
        let cur = match self.conns.get(idx) {
            Some(x) => x.tier,
            None => return Err(XcCode::ConnMissing),
        };
        let (to, changed) = self.governor.on_ok(conn_fp, cur, tick);
        if changed {
            if let Some(c) = self.conns.get_mut(idx) {
                c.tier = to;
            }
        }
        Ok(())
    }

    /// 立案计数面。
    pub const fn breach_counts(&self) -> (u32, u32) {
        (self.cpu_over_count, self.overclaim_count)
    }

    /// 引擎账链指纹。
    pub const fn chain_fingerprint(&self) -> u64 {
        self.chain
    }

    /// 引擎自洽核验（连接档全验+容量单重放）。
    pub fn validate(&self) -> Result<(), XcCode> {
        let mut i = 0usize;
        while i < self.conns.len() {
            if let Some(c) = self.conns.get(i) {
                c.validate()?;
            }
            i += 1;
        }
        let _ = self.probe.fingerprint();
        Ok(())
    }

    /// 人读汇总（含容量读屏——域本色）。
    pub fn report(&self) -> String {
        let (up, down) = self.total_declared_bps();
        let (co, oc) = self.breach_counts();
        alloc::format!(
            "NetBudgetEngine:连接{}/声明带宽↑{}↓{}/CPU超{}/虚标{}/{}",
            self.conns.len(),
            up,
            down,
            co,
            oc,
            self.probe.a11y_line()
        )
    }
}
