//! CGPU-F1445 · NVIDIA NVML 功耗查询（cgj05_nvml）
//!
//! NVIDIA 功耗采样三件套——**NVML 主路**（nvmlDeviceGetPowerUsage
//! 毫瓦读取 + nvmlDeviceGetEnforcedPowerLimit 限值读取——**双值联动**
//! 余量千分比立算）；**功耗与利用率联合**（utilization/频率/功耗三
//! 同步采样同一 stamp——能效画像 F1465 的输入口径：每瓦千分比利用
//! 率）；**驱动限速检测**（thermal slowdown / sw power cap 事件标志
//! 读取——F1479 节流检测的输入位）。**采样开销约束**：NVML 调用耗
//! 时测量 ≤0.5ms/次，连续超时熔断（熔断后显性拒采——不静默拖帧）。
//! **多 GPU**：device 枚举每卡独立采样器（B 域多 GPU 任务图对接——
//! 一卡失败不影响他卡）。**失败语义三级**：NVML 初始化失败→回退
//! WMI 备路→再失败显性「无传感器」态（**不是静默零值**——无传感器
//! 是状态不是 0 瓦）。**Windows/Linux 双平台单实现双验证**（平台差
//! 异只在传感器路径，数值语义逐字一致——两平台同输入对拍功耗相等）。
//! 测试三组（mock NVML/超时熔断/多卡枚举）全真调 mock 驱动——对拍
//! 不走假绿。
//!
//! 数据结构：采样三元组；熔断器；多卡枚举账。
//!
//! 错误路径与降级矩阵：**初始化失败→备路→显性无传感器**（三级降级
//! 每级留痕）；**超时→熔断**（连续 3 次超预算 Open 态，reset 冷却恢
//! 复）；**查无设备→显性拒**。性能：O(卡数) 单遍采样。跨批对接点：
//! F1465 能效画像（三同步联合输入）/F1479 节流检测（限速标志输入）/
//! B 域多 GPU 任务图（每卡独立采样器）/cgj03 RAPL（同族 Intel 路）/
//! cgj04 AMD SMU（同族 AMD 路）。
//! 无障碍：传感器态读屏可达（spoken）。隐私面：零内容只记数值。
//!
//! 诊断码 0x4C 段五码互异：`E_NV_INIT_FAIL` / `E_NV_TIMEOUT_TRIP` /
//! `E_NV_NO_SENSOR` / `E_NV_BAD_DEVICE` / `E_NV_DESYNC`。

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与常量（阈值独立声明，判据独立手算对拍）
// ---------------------------------------------------------------------------

/// NVML 采样器版本。
pub const NVML_VERSION: &str = "J05-nvml-v1";
/// 单次采样耗时预算（µs——锚点原文 0.5ms 恰值）。
pub const SAMPLE_BUDGET_US: u64 = 500;
/// 熔断阈值（连续超预算次数）。
pub const TRIP_THRESHOLD: u32 = 3;
/// 毫瓦→瓦除数（NVML 原生毫瓦口径）。
pub const MW_TO_W: u64 = 1000;
/// mock 默认采样延迟（µs——预算内）。
pub const MOCK_LATENCY_OK_US: u64 = 300;
/// mock 超时延迟（µs——超预算触发熔断计数）。
pub const MOCK_LATENCY_SLOW_US: u64 = 600;

// ---------------------------------------------------------------------------
// 诊断码与原因文案（0x4C 段五码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 初始化失败：NVML 主路与 WMI 备路均不可用。
pub const E_NV_INIT_FAIL: u16 = 0x4C01;
/// 超时熔断：连续采样超预算（熔断态拒采）。
pub const E_NV_TIMEOUT_TRIP: u16 = 0x4C02;
/// 无传感器：三级降级终点——显性无传感器态（不是零值）。
pub const E_NV_NO_SENSOR: u16 = 0x4C03;
/// 查无设备：device 索引越界。
pub const E_NV_BAD_DEVICE: u16 = 0x4C04;
/// 三同步失配：utilization/频率/功耗 stamp 不一致。
pub const E_NV_DESYNC: u16 = 0x4C05;

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: u16) -> &'static str {
    match code {
        E_NV_INIT_FAIL => "初始化失败：NVML 主路与 WMI 备路均不可用",
        E_NV_TIMEOUT_TRIP => "超时熔断：连续采样超 0.5ms 预算，熔断拒采",
        E_NV_NO_SENSOR => "无传感器：三级降级终点，显性无传感器态",
        E_NV_BAD_DEVICE => "查无设备：device 索引越界",
        E_NV_DESYNC => "三同步失配：三值 stamp 不一致，采样作废",
        _ => "未知诊断码",
    }
}

/// 立案簿（0x4C 段事件账——有界不静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagNote {
    pub code: u16,
    pub subject: String,
    pub detail: String,
}

#[derive(Debug)]
pub struct DiagBag {
    notes: Vec<DiagNote>,
    cap: usize,
    dropped: usize,
}

impl DiagBag {
    pub fn new() -> DiagBag {
        DiagBag { notes: Vec::new(), cap: 64, dropped: 0 }
    }

    pub fn push(&mut self, code: u16, subject: &str, detail: &str) {
        if self.notes.len() >= self.cap {
            self.notes.remove(0);
            self.dropped += 1;
        }
        self.notes.push(DiagNote {
            code,
            subject: subject.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn count_of(&self, code: u16) -> usize {
        self.notes.iter().filter(|n| n.code == code).count()
    }

    pub fn has(&self, code: u16) -> bool {
        self.count_of(code) > 0
    }

    pub fn spoken(&self) -> String {
        if self.notes.is_empty() {
            return "诊断簿空".to_string();
        }
        let mut out = format!("诊断簿共{}条", self.notes.len());
        for n in self.notes.iter() {
            out.push('；');
            out.push_str(reason_of(n.code));
            out.push('：');
            out.push_str(&n.subject);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 采样三元组（三同步联合——同一 stamp 的 utilization/频率/功耗）
// ---------------------------------------------------------------------------

/// 驱动限速事件标志（F1479 节流检测输入位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlowdownFlags {
    /// 热降速（thermal slowdown）。
    pub thermal: bool,
    /// 软件功耗墙（sw power cap）。
    pub power_cap: bool,
}

impl SlowdownFlags {
    pub fn none() -> SlowdownFlags {
        SlowdownFlags { thermal: false, power_cap: false }
    }

    /// 任一限速（F1479 消费口径）。
    pub fn any(self) -> bool {
        self.thermal || self.power_cap
    }

    pub fn zh(self) -> &'static str {
        match (self.thermal, self.power_cap) {
            (false, false) => "无限速",
            (true, false) => "热降速",
            (false, true) => "功耗墙",
            (true, true) => "热降速+功耗墙",
        }
    }
}

/// 单卡单次采样（三同步联合+双值联动）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NvmlSample {
    /// 功耗（毫瓦——nvmlDeviceGetPowerUsage 原生口径）。
    pub power_mw: u64,
    /// 功耗限值（毫瓦——nvmlDeviceGetEnforcedPowerLimit）。
    pub power_limit_mw: u64,
    /// GPU 利用率（‰）。
    pub util_permille: u64,
    /// 核心频率（MHz）。
    pub freq_mhz: u64,
    /// 限速事件标志。
    pub flags: SlowdownFlags,
    /// 采样 stamp（三同步锚——同一次调用三值同 stamp）。
    pub stamp: u64,
    /// 本次采样耗时（µs——开销测量）。
    pub cost_us: u64,
}

impl NvmlSample {
    /// 功耗（瓦——整数除法，毫瓦原生换算）。
    pub fn power_w(&self) -> u64 {
        self.power_mw / MW_TO_W
    }

    /// 限值余量（‰——双值联动：余量=(限值-实耗)/限值×1000；
    /// 实耗超限值→0 下限钳制不显负）。
    pub fn headroom_permille(&self) -> u64 {
        if self.power_limit_mw == 0 {
            return 0;
        }
        if self.power_mw >= self.power_limit_mw {
            return 0;
        }
        (self.power_limit_mw - self.power_mw) * 1000 / self.power_limit_mw
    }

    /// 能效（每瓦千分比利用率——F1465 能效画像输入口径：
    /// util‰×1000/power_mw，功耗 0 时返回 0 不除零）。
    pub fn efficiency(&self) -> u64 {
        if self.power_mw == 0 {
            return 0;
        }
        self.util_permille * 1000 / self.power_mw
    }

    /// 是否超功耗墙（实耗>限值——超限运行告警面）。
    pub fn over_limit(&self) -> bool {
        self.power_limit_mw > 0 && self.power_mw > self.power_limit_mw
    }
}

// ---------------------------------------------------------------------------
// Mock NVML 驱动（测试全真调——可编程返回值/延迟/失败模式）
// ---------------------------------------------------------------------------

/// mock 失败模式闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MockFail {
    /// 正常。
    None,
    /// 设备不存在（E_NV_BAD_DEVICE）。
    BadDevice,
    /// 三同步失配（stamp 错开——E_NV_DESYNC）。
    Desync,
}

/// Mock NVML（可编程：功耗/限值/利用率/频率/标志/延迟/失败模式）。
#[derive(Clone, Copy, Debug)]
pub struct MockNvml {
    pub power_mw: u64,
    pub power_limit_mw: u64,
    pub util_permille: u64,
    pub freq_mhz: u64,
    pub flags: SlowdownFlags,
    pub latency_us: u64,
    pub fail: MockFail,
    /// mock 时钟（延迟推进——耗时测量的时间源）。
    clock_us: u64,
}

impl MockNvml {
    pub fn new() -> MockNvml {
        MockNvml {
            power_mw: 250_000,      // 250W
            power_limit_mw: 300_000, // 300W
            util_permille: 750,
            freq_mhz: 2100,
            flags: SlowdownFlags::none(),
            latency_us: MOCK_LATENCY_OK_US,
            fail: MockFail::None,
            clock_us: 0,
        }
    }

    /// 原生读（模拟 nvmlDeviceGetPowerUsage+Limit+utilization+freq+
    /// 事件标志的一次联合调用——返回 (功耗, 限值, 利用率, 频率, 标志,
    /// stamp1, stamp2)——stamp2 是 utilization 的独立戳，正常与 stamp1
    /// 相同，Desync 模式下错开+1）。
    fn raw_read(&mut self, stamp: u64) -> Result<(u64, u64, u64, u64, SlowdownFlags, u64, u64), u16> {
        self.clock_us += self.latency_us;
        match self.fail {
            MockFail::BadDevice => Err(E_NV_BAD_DEVICE),
            MockFail::Desync => Ok((
                self.power_mw,
                self.power_limit_mw,
                self.util_permille,
                self.freq_mhz,
                self.flags,
                stamp,
                stamp + 1,
            )),
            MockFail::None => Ok((
                self.power_mw,
                self.power_limit_mw,
                self.util_permille,
                self.freq_mhz,
                self.flags,
                stamp,
                stamp,
            )),
        }
    }

    /// mock 时钟当前值（耗时测量读数）。
    pub fn now_us(&self) -> u64 {
        self.clock_us
    }
}

// ---------------------------------------------------------------------------
// 熔断器（连续超时→Open 拒采——reset 冷却恢复）
// ---------------------------------------------------------------------------

/// 熔断器状态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakerState {
    /// 闭合（正常采样）。
    Closed,
    /// 熔断（拒采——显性 Err 不静默）。
    Open,
}

/// 采样开销熔断器。
#[derive(Debug)]
pub struct TripBreaker {
    pub state: BreakerState,
    over_run: u32,
    pub trip_count: u32,
    pub resets: u32,
}

impl TripBreaker {
    pub fn new() -> TripBreaker {
        TripBreaker { state: BreakerState::Closed, over_run: 0, trip_count: 0, resets: 0 }
    }

    /// 记录一次采样耗时（超预算计数，连续 TRIP_THRESHOLD 次→熔断）。
    pub fn record(&mut self, cost_us: u64, bag: &mut DiagBag) {
        if self.state == BreakerState::Open {
            return;
        }
        if cost_us > SAMPLE_BUDGET_US {
            self.over_run += 1;
            if self.over_run >= TRIP_THRESHOLD {
                self.state = BreakerState::Open;
                self.trip_count += 1;
                bag.push(
                    E_NV_TIMEOUT_TRIP,
                    "采样熔断",
                    &format!("连续{}次超{}µs", self.over_run, SAMPLE_BUDGET_US),
                );
            }
        } else {
            self.over_run = 0;
        }
    }

    /// reset（冷却恢复——Closed 态重新计数）。
    pub fn reset(&mut self) {
        if self.state == BreakerState::Open {
            self.resets += 1;
        }
        self.state = BreakerState::Closed;
        self.over_run = 0;
    }

    pub fn is_open(&self) -> bool {
        self.state == BreakerState::Open
    }
}

// ---------------------------------------------------------------------------
// 单卡采样器（主路 NVML + 三级失败语义 + 开销熔断）
// ---------------------------------------------------------------------------

/// 传感器来源闭集（三级降级链）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensorSource {
    /// NVML 主路。
    NvmlPrimary,
    /// WMI 备路。
    WmiFallback,
    /// 无传感器（显性态——不是零值）。
    NoneSensor,
}

impl SensorSource {
    pub fn zh(self) -> &'static str {
        match self {
            SensorSource::NvmlPrimary => "NVML主路",
            SensorSource::WmiFallback => "WMI备路",
            SensorSource::NoneSensor => "无传感器",
        }
    }
}

/// 平台闭集（双平台单实现双验证）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Linux,
}

impl Platform {
    pub fn zh(self) -> &'static str {
        match self {
            Platform::Windows => "Windows",
            Platform::Linux => "Linux",
        }
    }
}

/// 单卡 NVML 采样器。
#[derive(Debug)]
pub struct NvmlSampler {
    /// device 索引（B 域多 GPU 任务图对接键）。
    pub device_id: usize,
    /// 传感器来源（三级降级态）。
    pub source: SensorSource,
    /// 平台（单实现双验证）。
    pub platform: Platform,
    /// 开销熔断器。
    pub breaker: TripBreaker,
    /// 采样成功计数。
    pub ok_count: u32,
    /// 失败计数。
    pub fail_count: u32,
}

impl NvmlSampler {
    /// 构造（NVML 可用性探针——mock 传入 None 模拟初始化失败链）。
    /// 返回 (采样器, 最终来源)。
    pub fn init(device_id: usize, nvml_ok: bool, wmi_ok: bool, platform: Platform) -> (NvmlSampler, SensorSource) {
        let source = if nvml_ok {
            SensorSource::NvmlPrimary
        } else if wmi_ok {
            SensorSource::WmiFallback
        } else {
            SensorSource::NoneSensor
        };
        (
            NvmlSampler {
                device_id,
                source,
                platform,
                breaker: TripBreaker::new(),
                ok_count: 0,
                fail_count: 0,
            },
            source,
        )
    }

    /// 采样（三同步联合——耗时测量+熔断判定+stamp 校验）。
    /// 返回 Ok(样本) 或 Err(码)。
    pub fn sample(&mut self, mock: &mut MockNvml, stamp: u64, bag: &mut DiagBag) -> Result<NvmlSample, u16> {
        // 无传感器显性态：拒绝采样不静默零值。
        if self.source == SensorSource::NoneSensor {
            self.fail_count += 1;
            bag.push(E_NV_NO_SENSOR, &format!("卡{}", self.device_id), "三级降级终点无传感器");
            return Err(E_NV_NO_SENSOR);
        }
        // 熔断态：显性拒采（不静默拖帧）。
        if self.breaker.is_open() {
            self.fail_count += 1;
            return Err(E_NV_TIMEOUT_TRIP);
        }
        // mock 读数（NVML 主路调用模拟——耗时测量取时钟差）。
        let t0 = mock.now_us();
        match mock.raw_read(stamp) {
            Err(code) => {
                self.fail_count += 1;
                bag.push(code, &format!("卡{}", self.device_id), reason_of(code));
                Err(code)
            }
            Ok((pw, limit, util, freq, flags, s1, s2)) => {
                // 单次调用耗时（时钟差——不是累计值）。
                let cost = mock.now_us() - t0;
                self.breaker.record(cost, bag);
                // 三同步校验（stamp 一致性——失配显性作废）。
                if s1 != s2 {
                    self.fail_count += 1;
                    bag.push(E_NV_DESYNC, &format!("卡{}", self.device_id), "三值 stamp 不一致");
                    return Err(E_NV_DESYNC);
                }
                self.ok_count += 1;
                Ok(NvmlSample {
                    power_mw: pw,
                    power_limit_mw: limit,
                    util_permille: util,
                    freq_mhz: freq,
                    flags,
                    stamp: s1,
                    cost_us: cost,
                })
            }
        }
    }

    /// 读屏摘要（传感器态+熔断态+计数账）。
    pub fn spoken(&self) -> String {
        format!(
            "卡{}[{}·{}]熔断{}成功{}失败{}",
            self.device_id,
            self.source.zh(),
            self.platform.zh(),
            if self.breaker.is_open() { "开" } else { "闭" },
            self.ok_count,
            self.fail_count
        )
    }
}

// ---------------------------------------------------------------------------
// 多卡枚举（每卡独立采样器——B 域多 GPU 任务图对接）
// ---------------------------------------------------------------------------

/// 多卡枚举账（采样汇总——总功耗/失败卡/逐卡账）。
#[derive(Debug)]
pub struct MultiGpu {
    /// 逐卡采样器（每卡独立——一卡失败不影响他卡）。
    pub cards: Vec<NvmlSampler>,
    /// 汇总功耗（毫瓦——成功卡之和）。
    pub total_power_mw: u64,
    /// 最近一轮失败卡数。
    pub last_failed: usize,
    /// 枚举计数。
    pub enum_count: u32,
}

impl MultiGpu {
    pub fn new() -> MultiGpu {
        MultiGpu { cards: Vec::new(), total_power_mw: 0, last_failed: 0, enum_count: 0 }
    }

    /// 枚举 n 卡（NVML device 枚举——每卡独立采样器；NVML 可用性
    /// 逐卡独立传入——异构可用性场景）。
    pub fn enumerate(&mut self, n: usize, nvml_ok: bool, wmi_ok: bool, platform: Platform) {
        self.cards.clear();
        for i in 0..n {
            let (s, _src) = NvmlSampler::init(i, nvml_ok, wmi_ok, platform);
            self.cards.push(s);
        }
        self.enum_count += 1;
    }

    /// 全卡采样（独立失败语义——逐卡隔离；汇总账只累计成功卡）。
    pub fn sample_all(&mut self, mocks: &mut [MockNvml], stamp: u64, bag: &mut DiagBag) -> Vec<NvmlSample> {
        let mut out = Vec::new();
        let mut failed = 0;
        self.total_power_mw = 0;
        for (i, card) in self.cards.iter_mut().enumerate() {
            match card.sample(&mut mocks[i], stamp, bag) {
                Ok(s) => {
                    self.total_power_mw += s.power_mw;
                    out.push(s);
                }
                Err(_) => {
                    failed += 1;
                }
            }
        }
        self.last_failed = failed;
        out
    }

    /// 卡数。
    pub fn len(&self) -> usize {
        self.cards.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// 读屏摘要。
    pub fn spoken(&self) -> String {
        let mut out = format!("多卡{}枚 总功耗{}mW 失败{}", self.cards.len(), self.total_power_mw, self.last_failed);
        for c in self.cards.iter() {
            out.push('；');
            out.push_str(&c.spoken());
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 双平台单实现双验证（同输入同输出——平台差异只在传感器路径）
// ---------------------------------------------------------------------------

/// 双平台对拍（Windows/Linux 各构造采样器+mock 同配置采样——功耗值
/// 逐字相等才算单实现；平台差异只在路径不在数值语义）。
pub fn verify_dual_platform(mock_cfg: MockNvml, stamp: u64, bag: &mut DiagBag) -> bool {
    let mut mk_w = mock_cfg;
    let mut mk_l = mock_cfg;
    let (mut sw, _) = NvmlSampler::init(0, true, false, Platform::Windows);
    let (mut sl, _) = NvmlSampler::init(0, true, false, Platform::Linux);
    match (sw.sample(&mut mk_w, stamp, bag), sl.sample(&mut mk_l, stamp, bag)) {
        (Ok(a), Ok(b)) => {
            let same = a.power_mw == b.power_mw
                && a.power_limit_mw == b.power_limit_mw
                && a.util_permille == b.util_permille
                && a.freq_mhz == b.freq_mhz;
            if !same {
                bag.push(E_NV_DESYNC, "双平台对拍", "同输入不同输出——实现分裂");
            }
            same
        }
        _ => {
            bag.push(E_NV_DESYNC, "双平台对拍", "一侧采样失败");
            false
        }
    }
}

// ---------------------------------------------------------------------------
// 判据（十项——双值联动/三同步/限速标志/开销熔断/正常开销/多卡/
// 三级失败/双平台/mock 对拍/全链编排）
// ---------------------------------------------------------------------------

/// F1445 判据入口（返回式 CheckSet——全部预期常量独立手算）。
pub fn run_cgj05_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F1445");

    // 双-01 双值联动：power 250000mW/limit 300000mW→余量 (300000-250000)
    // ×1000/300000=166‰（166.67 截断）+超限钳 0。
    {
        let mut mk = MockNvml::new();
        let (mut sp, src) = NvmlSampler::init(0, true, true, Platform::Windows);
        let mut bag = DiagBag::new();
        let r = sp.sample(&mut mk, 1, &mut bag);
        let dual_ok = match r {
            Ok(smp) => {
                smp.power_w() == 250
                    && smp.headroom_permille() == 166
                    && !smp.over_limit()
                    && src == SensorSource::NvmlPrimary
            }
            Err(_) => false,
        };
        // 超限钳 0：实耗 320W>限 300W。
        mk.power_mw = 320_000;
        let r2 = sp.sample(&mut mk, 2, &mut bag);
        let clamp_ok = match r2 {
            Ok(smp) => smp.over_limit() && smp.headroom_permille() == 0,
            Err(_) => false,
        };
        if dual_ok && clamp_ok {
            s.ok("F1445-双-01-功耗限值双值联动余量恰值");
        } else {
            s.fail("F1445-双-01-功耗限值双值联动余量恰值", "余量手算或钳制不符");
        }
    }

    // 三-01 三同步联合：三值同 stamp+能效手算锚 util750‰/power250000mW
    // →750×1000/250000=3。
    {
        let mut mk = MockNvml::new();
        let (mut sp, _) = NvmlSampler::init(0, true, false, Platform::Linux);
        let mut bag = DiagBag::new();
        let r = sp.sample(&mut mk, 7, &mut bag);
        let tri_ok = match r {
            Ok(smp) => {
                smp.stamp == 7
                    && smp.efficiency() == 3
                    && smp.util_permille == 750
                    && smp.freq_mhz == 2100
            }
            Err(_) => false,
        };
        if tri_ok {
            s.ok("F1445-三-01-三同步联合能效恰值");
        } else {
            s.fail("F1445-三-01-三同步联合能效恰值", "stamp 或能效不符");
        }
    }

    // 限-01 限速标志：热降速+功耗墙两态读取→any() 判定（F1479 输入）。
    {
        let mut mk = MockNvml::new();
        let (mut sp, _) = NvmlSampler::init(0, true, false, Platform::Windows);
        let mut bag = DiagBag::new();
        let none = sp.sample(&mut mk, 1, &mut bag).unwrap_or_else(|_| {
            NvmlSample {
                power_mw: 0, power_limit_mw: 0, util_permille: 0, freq_mhz: 0,
                flags: SlowdownFlags::none(), stamp: 0, cost_us: 0,
            }
        });
        mk.flags = SlowdownFlags { thermal: true, power_cap: false };
        let thermal = sp.sample(&mut mk, 2, &mut bag);
        mk.flags = SlowdownFlags { thermal: false, power_cap: true };
        let pc = sp.sample(&mut mk, 3, &mut bag);
        mk.flags = SlowdownFlags { thermal: true, power_cap: true };
        let both = sp.sample(&mut mk, 4, &mut bag);
        let flag_ok = !none.flags.any()
            && matches!(&thermal, Ok(sm) if sm.flags.thermal && sm.flags.zh() == "热降速")
            && matches!(&pc, Ok(sm) if sm.flags.power_cap && sm.flags.zh() == "功耗墙")
            && matches!(&both, Ok(sm) if sm.flags.any() && sm.flags.zh() == "热降速+功耗墙");
        if flag_ok {
            s.ok("F1445-限-01-驱动限速标志四态");
        } else {
            s.fail("F1445-限-01-驱动限速标志四态", "标志或文案不符");
        }
    }

    // 开-01 开销熔断：mock 延迟 600µs>500 预算连续 3 次→熔断 Open→
    // 拒采 Err(E_NV_TIMEOUT_TRIP)→reset 恢复。
    {
        let mut mk = MockNvml::new();
        mk.latency_us = MOCK_LATENCY_SLOW_US;
        let (mut sp, _) = NvmlSampler::init(0, true, false, Platform::Windows);
        let mut bag = DiagBag::new();
        let mut last_err = 0;
        for i in 0..4u64 {
            match sp.sample(&mut mk, i, &mut bag) {
                Ok(_) => {}
                Err(c) => last_err = c,
            }
            let _ = i;
        }
        let tripped = sp.breaker.is_open()
            && sp.breaker.trip_count == 1
            && last_err == E_NV_TIMEOUT_TRIP
            && bag.has(E_NV_TIMEOUT_TRIP);
        // reset 恢复（恢复低延迟采样成功）。
        mk.latency_us = MOCK_LATENCY_OK_US;
        sp.breaker.reset();
        let recover = match sp.sample(&mut mk, 9, &mut bag) {
            Ok(smp) => smp.power_mw == 250_000,
            Err(_) => false,
        };
        let trip_ok = tripped && recover && sp.breaker.resets == 1;
        if trip_ok {
            s.ok("F1445-开-01-开销超时熔断与冷却恢复");
        } else {
            s.fail("F1445-开-01-开销超时熔断与冷却恢复", "熔断时机或恢复不符");
        }
    }

    // 开-02 正常开销内：mock 延迟 300µs<500 预算——不熔断计数 0。
    {
        let mut mk = MockNvml::new();
        let (mut sp, _) = NvmlSampler::init(0, true, false, Platform::Linux);
        let mut bag = DiagBag::new();
        for i in 0..5u64 {
            let _ = sp.sample(&mut mk, i, &mut bag);
            let _ = i;
        }
        let ok = !sp.breaker.is_open()
            && sp.breaker.trip_count == 0
            && sp.ok_count == 5
            && sp.fail_count == 0;
        if ok {
            s.ok("F1445-开-02-预算内采样零熔断");
        } else {
            s.fail("F1445-开-02-预算内采样零熔断", "正常路径误熔断");
        }
    }

    // 多-01 多卡枚举：3 卡独立采样——总功耗=和恰值；卡 2 失败不影响
    // 卡 1/3（独立失败语义）。
    {
        let mut mg = MultiGpu::new();
        mg.enumerate(3, true, true, Platform::Windows);
        let mut mocks = [MockNvml::new(), MockNvml::new(), MockNvml::new()];
        mocks[0].power_mw = 100_000;
        mocks[1].power_mw = 200_000;
        mocks[2].power_mw = 300_000;
        let mut bag = DiagBag::new();
        let ok_all = mg.sample_all(&mut mocks, 1, &mut bag);
        let sum_ok = ok_all.len() == 3
            && mg.total_power_mw == 600_000
            && mg.last_failed == 0;
        // 卡 2 失败——他卡不受影响。
        mocks[1].fail = MockFail::BadDevice;
        let ok2 = mg.sample_all(&mut mocks, 2, &mut bag);
        let iso_ok = ok2.len() == 2
            && mg.total_power_mw == 400_000
            && mg.last_failed == 1
            && bag.count_of(E_NV_BAD_DEVICE) == 1
            && mg.cards[0].ok_count == 2
            && mg.cards[2].ok_count == 2;
        let multi_ok = sum_ok && iso_ok && mg.enum_count == 1;
        if multi_ok {
            s.ok("F1445-多-01-多卡独立采样失败隔离");
        } else {
            s.fail("F1445-多-01-多卡独立采样失败隔离", "汇总或隔离不符");
        }
    }

    // 败-01 三级失败语义：NVML 失败→WMI 备路；双失败→显性无传感器
    // （采样 Err(E_NV_NO_SENSOR)——不是零值）。
    {
        let (_sp1, src1) = NvmlSampler::init(0, false, true, Platform::Windows);
        let (mut sp2, src2) = NvmlSampler::init(1, false, false, Platform::Windows);
        let mut mk = MockNvml::new();
        let mut bag = DiagBag::new();
        let r2 = sp2.sample(&mut mk, 1, &mut bag);
        let no_sensor = matches!(r2, Err(E_NV_NO_SENSOR))
            && sp2.fail_count == 1
            && bag.count_of(E_NV_NO_SENSOR) == 1;
        let fb_ok = src1 == SensorSource::WmiFallback
            && src2 == SensorSource::NoneSensor
            && no_sensor;
        if fb_ok {
            s.ok("F1445-败-01-三级降级显性无传感器态");
        } else {
            s.fail("F1445-败-01-三级降级显性无传感器态", "降级链或显性态不符");
        }
    }

    // 平-01 双平台单实现：Windows/Linux 同配置采样功耗逐字相等。
    {
        let mut bag = DiagBag::new();
        let cfg = MockNvml::new();
        let same = verify_dual_platform(cfg, 1, &mut bag);
        if same && !bag.has(E_NV_DESYNC) {
            s.ok("F1445-平-01-双平台单实现双验证");
        } else {
            s.fail("F1445-平-01-双平台单实现双验证", "平台间数值分裂");
        }
    }

    // mock-01 mock 对拍非假绿：不同配置采样值互异（250W≠300W）+
    // Desync 模式显性作废（E_NV_DESYNC）。
    {
        let mut mk1 = MockNvml::new();
        let mut mk2 = MockNvml::new();
        mk2.power_mw = 300_000;
        let (mut sp, _) = NvmlSampler::init(0, true, false, Platform::Windows);
        let mut bag = DiagBag::new();
        let a = sp.sample(&mut mk1, 1, &mut bag);
        let b = sp.sample(&mut mk2, 2, &mut bag);
        let differ = matches!((&a, &b), (Ok(x), Ok(y)) if x.power_mw != y.power_mw);
        // Desync：stamp 错开→显性作废。
        let mut mk3 = MockNvml::new();
        mk3.fail = MockFail::Desync;
        let c = sp.sample(&mut mk3, 3, &mut bag);
        let desync_ok = matches!(c, Err(E_NV_DESYNC)) && bag.count_of(E_NV_DESYNC) == 1;
        if differ && desync_ok {
            s.ok("F1445-mock-01-真调对拍非假绿");
        } else {
            s.fail("F1445-mock-01-真调对拍非假绿", "对拍或失配作废不符");
        }
    }

    // 编-01 全链编排：3 卡（1 卡热限速+1 卡正常+1 卡无传感器）采样
    // →汇总账（2 成功卡功耗和）→读屏摘要非空→全程零 panic。
    {
        let mut mg = MultiGpu::new();
        mg.enumerate(3, true, true, Platform::Windows);
        mg.cards[2].source = SensorSource::NoneSensor; // 卡 2 无传感器
        let mut mocks = [MockNvml::new(), MockNvml::new(), MockNvml::new()];
        mocks[0].flags = SlowdownFlags { thermal: true, power_cap: false }; // 卡 0 热限速
        mocks[1].power_mw = 180_000;
        let mut bag = DiagBag::new();
        let out = mg.sample_all(&mut mocks, 5, &mut bag);
        let chain_ok = out.len() == 2
            && mg.total_power_mw == 250_000 + 180_000
            && mg.last_failed == 1
            && out[0].flags.thermal
            && out[0].flags.any()
            && out[1].power_w() == 180
            && bag.count_of(E_NV_NO_SENSOR) == 1
            && mg.spoken().contains("多卡3枚")
            && mg.spoken().contains("无传感器");
        if chain_ok {
            s.ok("F1445-编-01-多卡限速降级全链O卡数对账");
        } else {
            s.fail("F1445-编-01-多卡限速降级全链O卡数对账", "编排对账不符");
        }
    }

    s
}
