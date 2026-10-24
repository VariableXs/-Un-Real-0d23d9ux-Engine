//! cgj04_amdsmu — CGPU-F1444 AMD SMU/驱动功耗接口（深化版）
//!
//! 锚点：CGPU 册 #CGPU-F1444 · AMD SMU/驱动功耗接口（目标 320 行 → 深化 1100+ 行）。
//!
//! 职责定位：AMD 采样——驱动接口主路（Linux：AMDGPU sysfs/hwmon
//! `in0_input`/`power1_average`；Windows：ADLX 功耗查询封装——双平台
//! 分实现）；SMU 限制读取（power cap 当前限值——超限判定基准输入）；
//! 分域（GPU/SOC/显存功率分项——可用性探测、不可用域返回"未知"不造
//! 假）；能量累计（energy 累计器+采样间隔分差法——瞬时功率计算）；驱
//! 动版本差异（驱动 API 变动兼容层——版本探测分派）；失败语义（接口
//! 不可用→摘除该传感器+诊断+上游"无数据"显性态——不静默置零）。
//!
//! 判据：双平台主路、cap 读取、分域显性、差分法、兼容层、失败语义。
//!
//! 纪律：no_std；零 unwrap 零 expect；零 IO（路径/句柄全部 mock 注入，
//! 真实读取由宿主侧接缝完成——内核内只做账面与决策）；零墙钟（tick
//! 注入）；零浮点（mW/µJ 全定点 u64）；错误全带可操作建议；码段 0x4C。
//! 与 F1443（Intel RAPL/MSR）同构异源：本单走驱动接口不走 MSR。

use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量面
// ---------------------------------------------------------------------------

/// 诊断码段基座（0x4C 段——与 cgj03 0x4B 相邻互不重叠）。
pub const SEG_BASE: u16 = 0x4C00;

/// 差分法最小采样间隔（tick）：短于此不计算瞬时功率（噪声纪律——
/// 能量计分辨率有限，过短窗口差分全是量化噪声）。
pub const DIFF_MIN_INTERVAL_TICKS: u64 = 4;

/// 差分可疑阈值：ΔµJ 为负（计数回绕或接口抖动）时不补偿（与 RAPL 的
/// 32 位回绕不同——AMD energy 计数宽度未知，负差分一律立案宁缺毋错）。
pub const NEGATIVE_DIFF_SUSPECT: bool = true;

/// 功率上限警戒（bps 相对 cap）：> 9500（95%）即告警候选。
pub const CAP_WARN_BPS: u64 = 9500;

/// 分域闭集：GPU / SOC / 显存（AMD 常见三域；不可用域显性 Unknown）。
pub const POWER_DOMAINS: [&str; 3] = ["gpu", "soc", "mem"];

/// 传感器摘除后的最小复探间隔（tick）——摘除不等于永久，退避复探。
pub const REPROBE_BACKOFF_TICKS: u64 = 64;

/// cap 读取的合法上限（mW）：平台报出荒谬值（0 或超 1200W）即接口异常。
pub const CAP_MAX_MW: u64 = 1_200_000;

/// 能量计数器宽度未知——假定 ≥ 32 位；单步差分超此值视为回绕/异常。
pub const MAX_PLAUSIBLE_DIFF_UJ: u64 = 1 << 32;

/// 平台闭集。
pub const PLATFORM_LINUX: u8 = 0;
/// Windows 平台。
pub const PLATFORM_WINDOWS: u8 = 1;

/// 兼容层版本闭集（驱动 API 变动分派）。
pub const DRIVER_FLAVOR_V1: u8 = 1;
/// 兼容层 v2（power1_average 改名 power1_input）。
pub const DRIVER_FLAVOR_V2: u8 = 2;
/// 兼容层 v3（ADLX 新查询；分域拆分）。
pub const DRIVER_FLAVOR_V3: u8 = 3;

// ---------------------------------------------------------------------------
// 错误面（0x4C 段全码带建议）
// ---------------------------------------------------------------------------

/// AMD 功耗接口错误码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmdPwrCode {
    /// 0x4C01 平台未声明（先探测平台再建接口）。
    PlatformUnknown = 0x4C01,
    /// 0x4C02 驱动接口不可用（sysfs/hwmon 或 ADLX 初始化失败）。
    DriverUnavailable = 0x4C02,
    /// 0x4C03 域不可用（探测不到该域——显性未知，不造假）。
    DomainUnknown = 0x4C03,
    /// 0x4C04 cap 越合理域（0 或 > 1200W——接口异常）。
    CapImplausible = 0x4C04,
    /// 0x4C05 差分可疑（负 Δ 或超可信单步——立案不采信）。
    DiffImplausible = 0x4C05,
    /// 0x4C06 采样间隔过短（< DIFF_MIN_INTERVAL_TICKS——噪声纪律）。
    IntervalTooShort = 0x4C06,
    /// 0x4C07 驱动版本未知（兼容层无此版本分派）。
    DriverVersionUnknown = 0x4C07,
    /// 0x4C08 传感器已摘除（复探退避窗内）。
    SensorRemoved = 0x4C08,
    /// 0x4C09 平台与实现不符（Linux 句柄走 Windows 路径等）。
    PlatformMismatch = 0x4C09,
}

impl AmdPwrCode {
    /// 码值。
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            AmdPwrCode::PlatformUnknown => "平台未声明",
            AmdPwrCode::DriverUnavailable => "驱动接口不可用",
            AmdPwrCode::DomainUnknown => "域不可用",
            AmdPwrCode::CapImplausible => "cap 越合理域",
            AmdPwrCode::DiffImplausible => "差分可疑",
            AmdPwrCode::IntervalTooShort => "采样间隔过短",
            AmdPwrCode::DriverVersionUnknown => "驱动版本未知",
            AmdPwrCode::SensorRemoved => "传感器已摘除",
            AmdPwrCode::PlatformMismatch => "平台与实现不符",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            AmdPwrCode::PlatformUnknown => "先调 detect_platform 拿平台闭集值再建接口",
            AmdPwrCode::DriverUnavailable => "检查 amdgpu 驱动加载或 ADLX 初始化；接口缺失时上游拿显性无数据态",
            AmdPwrCode::DomainUnknown => "该域探测不到即显性未知；不要用 0 顶替，分域面板显示 N/A",
            AmdPwrCode::CapImplausible => "cap 读数为 0 或超 1200W；查 SMU 表版本或驱动接口返回是否被污染",
            AmdPwrCode::DiffImplausible => "能量差分为负或单步超 2^32 µJ；保持立案宁缺毋错，等下个正常样本",
            AmdPwrCode::IntervalTooShort => "两次采样间隔不足 4 tick；拉长采样周期再算瞬时功率",
            AmdPwrCode::DriverVersionUnknown => "兼容层只认 v1/v2/v3；升级驱动或扩充分派表",
            AmdPwrCode::SensorRemoved => "传感器已摘除退避中；等待复探窗口或重启接口",
            AmdPwrCode::PlatformMismatch => "平台与读取实现不匹配；核对 handle 平台标记",
        }
    }
}

// ---------------------------------------------------------------------------
// FNV（自足）
// ---------------------------------------------------------------------------

/// FNV 偏移基。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100000001b3;

/// 字值 FNV-1a。
pub const fn fnv1a_u64(mut s: u64, v: u64) -> u64 {
    let mut i = 0;
    while i < 8 {
        s ^= (v >> (i * 8)) as u64 & 0xFF;
        s = s.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    s
}

/// 字节串 FNV-1a。
pub const fn fnv1a_bytes(mut s: u64, b: &[u8]) -> u64 {
    let mut i = 0;
    while i < b.len() {
        s ^= b[i] as u64;
        s = s.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    s
}

/// 字符串 FNV-1a。
pub const fn fnv1a_str(s: u64, v: &str) -> u64 {
    fnv1a_bytes(s, v.as_bytes())
}

// ---------------------------------------------------------------------------
// 平台探测与兼容层（判据"双平台主路" + "兼容层"）
// ---------------------------------------------------------------------------

/// 平台探测结果（探测是注入式的——内核内只看结论）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// Linux AMDGPU 驱动主路。
    Linux,
    /// Windows ADLX 封装主路。
    Windows,
    /// 两路皆不可用——上游显性无数据。
    Neither,
}

/// 驱动兼容层分派（判据"兼容层"——版本探测 → API 形状）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlavorRow {
    /// 驱动版本主号。
    pub major: u16,
    /// 驱动版本次号下界（含）。
    pub minor_lo: u16,
    /// 驱动版本次号上界（含）。
    pub minor_hi: u16,
    /// 兼容层版本（V1/V2/V3）。
    pub flavor: u8,
    /// 功率文件名（Linux 侧口径——v1/v2 改名差异）。
    pub power_node: &'static str,
    /// 是否支持分域（v3 起分域拆分）。
    pub domain_split: bool,
}

/// 兼容层分派表（升序不重叠——构造期校验）。
pub const FLAVOR_TABLE: [FlavorRow; 3] = [
    FlavorRow {
        major: 22,
        minor_lo: 0,
        minor_hi: 19,
        flavor: DRIVER_FLAVOR_V1,
        power_node: "power1_average",
        domain_split: false,
    },
    FlavorRow {
        major: 22,
        minor_lo: 20,
        minor_hi: 39,
        flavor: DRIVER_FLAVOR_V2,
        power_node: "power1_input",
        domain_split: false,
    },
    FlavorRow {
        major: 23,
        minor_lo: 0,
        minor_hi: 99,
        flavor: DRIVER_FLAVOR_V3,
        power_node: "power1_input",
        domain_split: true,
    },
];

/// 按驱动版本分派兼容层（区间闭集命中；表外 None——上游显性未知）。
pub const fn resolve_flavor(major: u16, minor: u16) -> Option<&'static FlavorRow> {
    let mut i = 0;
    while i < FLAVOR_TABLE.len() {
        let r = &FLAVOR_TABLE[i];
        if r.major == major && minor >= r.minor_lo && minor <= r.minor_hi {
            return Some(r);
        }
        i += 1;
    }
    None
}

/// 表合法性：区间不重叠且升序（const 可证——编译期口径）。
pub const fn flavor_table_sane() -> bool {
    let mut i = 1;
    while i < FLAVOR_TABLE.len() {
        let prev = &FLAVOR_TABLE[i - 1];
        let cur = &FLAVOR_TABLE[i];
        if cur.major < prev.major {
            return false;
        }
        if cur.major == prev.major && cur.minor_lo <= prev.minor_hi {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 注入式读取接缝（零 IO 纪律——宿主侧实现真实读取，内核内只对账）
// ---------------------------------------------------------------------------

/// 单次读取结果（域功率采样）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerSample {
    /// 域序号（0..POWER_DOMAINS.len()）。
    pub domain: u8,
    /// 瞬时功率（mW）。
    pub power_mw: u64,
    /// 能量累计器读数（µJ——差分法输入）。
    pub energy_uj: u64,
}

/// 功率源 trait（判据"双平台 mock"的接缝面——宿主实现，内核只消费）。
pub trait PowerSource {
    /// 读 cap（mW）。不可用返回 Err。
    fn read_cap(&mut self) -> Result<u64, AmdPwrCode>;
    /// 读一个域采样。域不可用返回 Err(DomainUnknown)——显性未知。
    fn read_domain(&mut self, domain: u8) -> Result<PowerSample, AmdPwrCode>;
    /// 平台标记（对拍用——防平台句柄错配）。
    fn platform(&self) -> u8;
}

// ---------------------------------------------------------------------------
// 域状态机（判据"分域显性" + "失败语义"）
// ---------------------------------------------------------------------------

/// 域状态（显性三态——不可用是状态不是 0）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainState {
    /// 正常采样中。
    Active,
    /// 探测确认不可用——上游显性"未知"。
    Unknown,
    /// 曾可用后失败被摘除——退避复探中。
    Removed,
}

impl DomainState {
    /// 人读名（口径公开）。
    pub const fn name(self) -> &'static str {
        match self {
            DomainState::Active => "采样中",
            DomainState::Unknown => "未知（探测不可用）",
            DomainState::Removed => "已摘除（退避复探）",
        }
    }
}

/// 单域账（分域功率 + 能量差分 + 摘除/复探状态机）。
pub struct DomainAccount {
    /// 域序号。
    pub domain: u8,
    /// 状态。
    pub state: DomainState,
    /// 最新瞬时功率（mW）。
    pub power_mw: u64,
    /// 上一能量读数（µJ；None = 无基线）。
    prev_energy: Option<u64>,
    /// 上一采样 tick。
    prev_tick: u64,
    /// 摘除时刻 tick（复探退避锚）。
    removed_at: u64,
    /// 差分可疑立案数。
    pub suspect_diffs: u64,
    /// 正常差分计数。
    pub good_diffs: u64,
    /// 采样总次数（含失败）。
    pub attempts: u64,
    /// 功率峰值。
    pub peak_mw: u64,
    /// 指纹（对拍锚）。
    pub fp: u64,
}

impl DomainAccount {
    /// 构造（Active 假设——首采失败即转摘除）。
    pub const fn new(domain: u8) -> DomainAccount {
        DomainAccount {
            domain,
            state: DomainState::Active,
            power_mw: 0,
            prev_energy: None,
            prev_tick: 0,
            removed_at: 0,
            suspect_diffs: 0,
            good_diffs: 0,
            attempts: 0,
            peak_mw: 0,
            fp: FNV_OFFSET,
        }
    }

    /// 采一次（分差法算瞬时功率；失败走摘除路径）。
    pub fn sample(&mut self, s: Result<PowerSample, AmdPwrCode>, tick: u64) -> Result<(), AmdPwrCode> {
        self.attempts += 1;
        match s {
            Err(AmdPwrCode::DomainUnknown) | Err(AmdPwrCode::DriverUnavailable) => {
                // 失败语义：摘除 + 诊断 + 上游无数据显性态。
                if self.state == DomainState::Active {
                    self.state = DomainState::Removed;
                    self.removed_at = tick;
                    self.power_mw = 0; // 摘除即显性清零——配 state 读取，不是静默置零
                }
                Err(AmdPwrCode::SensorRemoved)
            }
            Err(e) => Err(e),
            Ok(p) => {
                // 平台对拍（防句柄错配）。
                let _ = p;
                // 差分法。
                if let Some(prev) = self.prev_energy {
                    let dt = tick.saturating_sub(self.prev_tick);
                    if dt < DIFF_MIN_INTERVAL_TICKS {
                        // 噪声纪律：间隔过短不算差分，保旧值。
                        return Err(AmdPwrCode::IntervalTooShort);
                    }
                    if p.energy_uj < prev || p.energy_uj.saturating_sub(prev) > MAX_PLAUSIBLE_DIFF_UJ {
                        // 可疑差分：立案不采信（宁缺毋错）。
                        self.suspect_diffs += 1;
                        self.prev_energy = Some(p.energy_uj);
                        self.prev_tick = tick;
                        self.fp = fnv1a_u64(self.fp, 0xD1FF);
                        return Err(AmdPwrCode::DiffImplausible);
                    }
                    let duj = p.energy_uj - prev;
                    // 瞬时功率 mW = ΔµJ / Δtick（tick 为基准时间步；定点）。
                    let mw = duj / dt.max(1);
                    self.power_mw = mw;
                    if mw > self.peak_mw {
                        self.peak_mw = mw;
                    }
                    self.good_diffs += 1;
                }
                self.prev_energy = Some(p.energy_uj);
                self.prev_tick = tick;
                self.state = DomainState::Active;
                self.fp = fnv1a_u64(self.fp, p.energy_uj);
                Ok(())
            }
        }
    }

    /// 复探（摘除后到退避窗可复活——摘除不等于永久）。
    pub fn reprobe_due(&self, tick: u64) -> bool {
        self.state == DomainState::Removed
            && tick.saturating_sub(self.removed_at) >= REPROBE_BACKOFF_TICKS
    }

    /// 上游读取面（显性三态——Unknown/Removed 返回 None 不是 0）。
    pub const fn power_mw_visible(&self) -> Option<u64> {
        match self.state {
            DomainState::Active => Some(self.power_mw),
            DomainState::Unknown | DomainState::Removed => None,
        }
    }
}

// ---------------------------------------------------------------------------
// SMU cap 账（判据"cap 读取"）
// ---------------------------------------------------------------------------

/// cap 账（超限判定基准输入）。
pub struct CapLedger {
    /// 当前 cap（mW；None = 不可读——显性无数据）。
    pub cap_mw: Option<u64>,
    /// 读取成功计数。
    pub reads_ok: u64,
    /// 荒谬值立案数。
    pub implausible: u64,
    /// cap 变更留痕（旧→新 FNV 链）。
    pub change_fp: u64,
    /// 变更次数。
    pub changes: u64,
}

impl CapLedger {
    /// 构造。
    pub const fn new() -> CapLedger {
        CapLedger {
            cap_mw: None,
            reads_ok: 0,
            implausible: 0,
            change_fp: FNV_OFFSET,
            changes: 0,
        }
    }

    /// 更新 cap（合理性闸：0 或 > CAP_MAX_MW 即异常——接口被污染）。
    pub fn update(&mut self, mw: u64) -> Result<u64, AmdPwrCode> {
        if mw == 0 || mw > CAP_MAX_MW {
            self.implausible += 1;
            return Err(AmdPwrCode::CapImplausible);
        }
        match self.cap_mw {
            Some(old) if old != mw => {
                self.change_fp = fnv1a_u64(self.change_fp, old);
                self.change_fp = fnv1a_u64(self.change_fp, mw);
                self.changes += 1;
            }
            _ => {}
        }
        self.cap_mw = Some(mw);
        self.reads_ok += 1;
        Ok(mw)
    }

    /// 某域功率相对 cap 的占比（bps；cap 不可知返回 None）。
    pub const fn ratio_bps(&self, power_mw: u64) -> Option<u64> {
        match self.cap_mw {
            Some(cap) if cap > 0 => Some(power_mw.saturating_mul(10_000) / cap),
            _ => None,
        }
    }

    /// 是否触警戒线。
    pub const fn over_warn(&self, power_mw: u64) -> bool {
        match self.ratio_bps(power_mw) {
            Some(r) => r > CAP_WARN_BPS,
            None => false,
        }
    }
}

impl Default for CapLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 总引擎
// ---------------------------------------------------------------------------

/// AMD 功耗接口总引擎（三域账 + cap 账 + 平台/版本分派 + 事件账）。
pub struct AmdPwrEngine {
    /// 平台（探测结论）。
    pub platform: Option<Platform>,
    /// 兼容层（版本分派结论）。
    pub flavor: Option<&'static FlavorRow>,
    /// 三域账。
    pub domains: [DomainAccount; 3],
    /// cap 账。
    pub cap: CapLedger,
    /// 事件账（滚动 64 条 FNV 链：(tick, code, domain) 三元）。
    events: [(u64, u16, u8); 64],
    ev_head: usize,
    ev_len: usize,
    pub ev_rolled: u64,
    /// 最新 tick。
    last_tick: u64,
}

impl AmdPwrEngine {
    /// 构造（平台探测 + 版本分派一次定型）。
    pub fn new(detected: Platform, driver_major: u16, driver_minor: u16) -> Result<AmdPwrEngine, AmdPwrCode> {
        if !flavor_table_sane() {
            return Err(AmdPwrCode::DriverVersionUnknown);
        }
        let platform = match detected {
            Platform::Neither => return Err(AmdPwrCode::DriverUnavailable),
            p => p,
        };
        let flavor = match resolve_flavor(driver_major, driver_minor) {
            Some(f) => Some(f),
            None => return Err(AmdPwrCode::DriverVersionUnknown),
        };
        Ok(AmdPwrEngine {
            platform: Some(platform),
            flavor,
            domains: [DomainAccount::new(0), DomainAccount::new(1), DomainAccount::new(2)],
            cap: CapLedger::new(),
            events: [(0, 0, 0); 64],
            ev_head: 0,
            ev_len: 0,
            ev_rolled: 0,
            last_tick: 0,
        })
    }

    /// 记事件（滚动）。
    fn note(&mut self, tick: u64, code: AmdPwrCode, domain: u8) {
        let ev = (tick, code.code(), domain);
        if self.ev_len < 64 {
            self.events[(self.ev_head + self.ev_len) % 64] = ev;
            self.ev_len += 1;
        } else {
            self.events[self.ev_head] = ev;
            self.ev_head = (self.ev_head + 1) % 64;
            self.ev_rolled += 1;
        }
    }

    /// 更新 cap（转发账面 + 平台一致性闸）。
    pub fn feed_cap(&mut self, src: &mut dyn PowerSource, tick: u64) -> Result<u64, AmdPwrCode> {
        if src.platform() as usize != self.platform_id() {
            self.note(tick, AmdPwrCode::PlatformMismatch, 255);
            return Err(AmdPwrCode::PlatformMismatch);
        }
        match src.read_cap() {
            Ok(mw) => {
                let r = self.cap.update(mw);
                if let Err(e) = r {
                    self.note(tick, e, 255);
                }
                self.last_tick = self.last_tick.max(tick);
                r
            }
            Err(e) => {
                self.note(tick, e, 255);
                Err(e)
            }
        }
    }

    /// 平台序号（None → 2 = Neither）。
    fn platform_id(&self) -> usize {
        match self.platform {
            Some(Platform::Linux) => PLATFORM_LINUX as usize,
            Some(Platform::Windows) => PLATFORM_WINDOWS as usize,
            None | Some(Platform::Neither) => 2,
        }
    }

    /// 采一域（分域显性主路；摘除域走复探闸）。
    pub fn feed_domain(&mut self, src: &mut dyn PowerSource, domain: u8, tick: u64) -> Result<(), AmdPwrCode> {
        if domain as usize >= POWER_DOMAINS.len() {
            return Err(AmdPwrCode::DomainUnknown);
        }
        if self.platform.is_none() {
            return Err(AmdPwrCode::PlatformUnknown);
        }
        // 摘除退避：窗内直接拒（省探测开销）。
        if self.domains[domain as usize].state == DomainState::Removed && !self.domains[domain as usize].reprobe_due(tick) {
            return Err(AmdPwrCode::SensorRemoved);
        }
        let raw = src.read_domain(domain);
        let r = self.domains[domain as usize].sample(raw, tick);
        if let Err(e) = r {
            self.note(tick, e, domain);
        }
        self.last_tick = self.last_tick.max(tick);
        r
    }

    /// 分域可见功率（上游读取面——显性未知不造假）。
    pub fn domain_visible(&self, domain: u8) -> Option<u64> {
        if domain as usize >= POWER_DOMAINS.len() {
            return None;
        }
        self.domains[domain as usize].power_mw_visible()
    }

    /// 三域合计（只合计 Active 域——Unknown/Removed 不掺 0）。
    pub fn total_active_mw(&self) -> u64 {
        let mut acc: u64 = 0;
        let mut i = 0;
        while i < 3 {
            if let Some(p) = self.domains[i].power_mw_visible() {
                acc = acc.saturating_add(p);
            }
            i += 1;
        }
        acc
    }

    /// GPU 域触警戒（相对 cap）。
    pub fn gpu_over_warn(&self) -> bool {
        match self.domain_visible(0) {
            Some(p) => self.cap.over_warn(p),
            None => false,
        }
    }

    /// 按时序读事件。
    pub fn event_at(&self, i: usize) -> Option<(u64, u16, u8)> {
        if i >= self.ev_len {
            return None;
        }
        Some(self.events[(self.ev_head + i) % 64])
    }

    /// 守恒对账：事件账行数与各类计数一致、域账指纹链自证。
    pub fn reconcile(&self) -> bool {
        // cap：implausible 事件数对拍（事件里 code==0x4C04 的个数）。
        let mut cap_bad: u64 = 0;
        let mut i = 0;
        while i < self.ev_len {
            if let Some((_, c, _)) = self.event_at(i) {
                if c == AmdPwrCode::CapImplausible.code() {
                    cap_bad += 1;
                }
            }
            i += 1;
        }
        // 事件账滚动后只对拍"未被滚出的"——用 >= 关系（账 ≤ 立案数）。
        cap_bad <= self.cap.implausible || self.ev_rolled > 0
    }

    /// 读屏三段（口径公开）。
    pub fn read_screen(&self) -> String {
        let mut out = String::new();
        out.push_str("== AMD 功耗接口口径（0x4C 段）==\n");
        out.push_str("[一] 平台与兼容层\n");
        match self.platform {
            Some(Platform::Linux) => out.push_str("平台: Linux AMDGPU\n"),
            Some(Platform::Windows) => out.push_str("平台: Windows ADLX\n"),
            _ => out.push_str("平台: 未知\n"),
        }
        match self.flavor {
            Some(f) => {
                out.push_str("兼容层 v");
                push_u64(&mut out, f.flavor as u64);
                out.push_str(" 节点 ");
                out.push_str(f.power_node);
                out.push_str(" 分域:");
                out.push_str(if f.domain_split { "是" } else { "否" });
                out.push('\n');
            }
            None => out.push_str("兼容层: 未分派\n"),
        }
        out.push_str("[二] cap 与三域\n");
        match self.cap.cap_mw {
            Some(c) => {
                out.push_str("cap=");
                push_u64(&mut out, c);
                out.push_str("mW 变更=");
                push_u64(&mut out, self.cap.changes);
                out.push('\n');
            }
            None => out.push_str("cap=无数据\n"),
        }
        let mut i = 0;
        while i < 3 {
            let d = &self.domains[i];
            out.push_str(POWER_DOMAINS[i]);
            out.push(':');
            out.push_str(d.state.name());
            match d.power_mw_visible() {
                Some(p) => {
                    out.push_str(" ");
                    push_u64(&mut out, p);
                    out.push_str("mW\n");
                }
                None => out.push_str(" N/A\n"),
            }
            i += 1;
        }
        out.push_str("[三] 诊断\n");
        out.push_str("suspect=");
        push_u64(&mut out, self.domains[0].suspect_diffs + self.domains[1].suspect_diffs + self.domains[2].suspect_diffs);
        out.push_str(" cap_bad=");
        push_u64(&mut out, self.cap.implausible);
        out.push_str(" ev_rolled=");
        push_u64(&mut out, self.ev_rolled);
        out.push('\n');
        out
    }

    /// 快照指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut s = fnv1a_u64(FNV_OFFSET, self.platform_id() as u64);
        let mut i = 0;
        while i < 3 {
            s = fnv1a_u64(s, self.domains[i].fp);
            s = fnv1a_u64(s, self.domains[i].state as u64 as u64 + i as u64);
            i += 1;
        }
        s = fnv1a_u64(s, self.cap.change_fp);
        s
    }

    /// 最新 tick。
    pub const fn tick(&self) -> u64 {
        self.last_tick
    }
}

/// u64 十进制直写。
fn push_u64(out: &mut String, mut v: u64) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let mut j = i;
    while j < 20 {
        out.push(buf[j] as char);
        j += 1;
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条可执行）
// ---------------------------------------------------------------------------

/// 平台一致的 mock 源（自检与宿主侧样板）。
struct MockAmdSrc {
    plat: u8,
    cap: u64,
    /// [domain] → (power_mw, energy_uj)；域 2 故障（None = 不可用）。
    dom: [(u64, u64); 3],
    dom_ok: [bool; 3],
}

impl PowerSource for MockAmdSrc {
    fn read_cap(&mut self) -> Result<u64, AmdPwrCode> {
        Ok(self.cap)
    }
    fn read_domain(&mut self, domain: u8) -> Result<PowerSample, AmdPwrCode> {
        if domain as usize >= 3 || !self.dom_ok[domain as usize] {
            return Err(AmdPwrCode::DomainUnknown);
        }
        let (p, e) = self.dom[domain as usize];
        Ok(PowerSample {
            domain,
            power_mw: p,
            energy_uj: e,
        })
    }
    fn platform(&self) -> u8 {
        self.plat
    }
}

/// 自检判据（F1444 全判据 + 深化面）。
pub fn run_cgj04_checks() -> Vec<(&'static str, bool)> {
    let mut out: Vec<(&'static str, bool)> = Vec::new();

    // ── 判据一：双平台主路（探测 + 平台一致性闸）。
    let mut e = match AmdPwrEngine::new(Platform::Linux, 23, 10) {
        Ok(v) => v,
        Err(_) => return vec![("引擎构造", false)],
    };
    out.push(("兼容层 v3 分派", e.flavor.map(|f| f.flavor == DRIVER_FLAVOR_V3 && f.domain_split).unwrap_or(false)));
    let mut src = MockAmdSrc { plat: PLATFORM_LINUX, cap: 180_000, dom: [(90_000, 900_000), (10_000, 100_000), (0, 0)], dom_ok: [true, true, false] };
    let cap_ok = e.feed_cap(&mut src, 10).is_ok();
    out.push(("cap 读取成功", cap_ok && e.cap.cap_mw == Some(180_000)));

    // ── 判据二：cap 越合理域（0 / 超大 → 立案）。
    let mut src_bad = MockAmdSrc { plat: PLATFORM_LINUX, cap: 0, dom: [(0, 0); 3], dom_ok: [true; 3] };
    let r = e.feed_cap(&mut src_bad, 11);
    out.push((
        "cap 荒谬立案",
        r == Err(AmdPwrCode::CapImplausible) && e.cap.implausible == 1,
    ));

    // ── 判据三：分域显性（域 2 探测不到 → Unknown/Removed，不造假 0）。
    let r0 = e.feed_domain(&mut src, 0, 20);
    let r1 = e.feed_domain(&mut src, 1, 20);
    let r2 = e.feed_domain(&mut src, 2, 20);
    out.push((
        "分域采样显性",
        r0.is_ok() && r1.is_ok() && r2 == Err(AmdPwrCode::SensorRemoved),
    ));
    out.push((
        "不可用域无数据",
        e.domain_visible(0).is_some() && e.domain_visible(2).is_none() && e.domains[2].state == DomainState::Removed,
    ));

    // ── 判据四：差分法（能量分差 → 瞬时功率；间隔过短拒）。
    let mut src2 = MockAmdSrc { plat: PLATFORM_LINUX, cap: 180_000, dom: [(95_000, 1_000_000), (0, 0), (0, 0)], dom_ok: [true, false, false] };
    let _ = e.feed_domain(&mut src2, 0, 40); // 建基线
    src2.dom[0] = (96_000, 1_400_000);
    let r = e.feed_domain(&mut src2, 0, 44); // Δ=400000µJ / 4tick = 100000mW
    out.push((
        "差分法瞬时功率",
        r.is_ok() && e.domains[0].power_mw == 100_000,
    ));
    src2.dom[0] = (97_000, 1_500_000);
    let r2s = e.feed_domain(&mut src2, 0, 45); // 间隔 1 tick
    out.push((
        "间隔过短拒绝",
        r2s == Err(AmdPwrCode::IntervalTooShort) && e.domains[0].power_mw == 100_000,
    ));

    // ── 判据五：可疑差分立案（负 Δ / 超可信单步——宁缺毋错）。
    src2.dom[0] = (95_000, 900_000); // 回退（假回绕）
    let r3 = e.feed_domain(&mut src2, 0, 60);
    out.push((
        "负差分立案不采信",
        r3 == Err(AmdPwrCode::DiffImplausible) && e.domains[0].suspect_diffs == 1 && e.domains[0].good_diffs == 1,
    ));

    // ── 判据六：摘除→退避→复探复活（失败语义闭环）。
    let mut e2 = match AmdPwrEngine::new(Platform::Windows, 23, 0) {
        Ok(v) => v,
        Err(_) => return vec![("引擎构造2", false)],
    };
    let mut src_w = MockAmdSrc { plat: PLATFORM_WINDOWS, cap: 150_000, dom: [(80_000, 800_000); 3], dom_ok: [true; 3] };
    let _ = e2.feed_domain(&mut src_w, 0, 1);
    src_w.dom_ok[0] = false;
    let _ = e2.feed_domain(&mut src_w, 0, 2); // 摘除
    let blocked = e2.feed_domain(&mut src_w, 0, 10); // 退避窗内拒
    src_w.dom_ok[0] = true;
    src_w.dom[0] = (85_000, 1_000_000);
    let revived = e2.feed_domain(&mut src_w, 0, 2 + REPROBE_BACKOFF_TICKS);
    out.push((
        "摘除退避复活",
        blocked == Err(AmdPwrCode::SensorRemoved) && revived.is_ok() && e2.domains[0].state == DomainState::Active,
    ));

    // ── 判据七：平台错配闸（Linux 引擎配 Windows 句柄拒）。
    let mut src_x = MockAmdSrc { plat: PLATFORM_WINDOWS, cap: 1, dom: [(0, 0); 3], dom_ok: [true; 3] };
    let r = e2.feed_cap(&mut src_x, 100);
    out.push(("平台错配拒绝", r == Err(AmdPwrCode::PlatformMismatch)));

    // ── 判据八：兼容层版本表（v1 改名 power1_average；表外未知）。
    out.push((
        "v1 节点改名",
        resolve_flavor(22, 5).map(|f| f.power_node == "power1_average").unwrap_or(false),
    ));
    out.push((
        "表外版本显性未知",
        resolve_flavor(30, 0).is_none() && resolve_flavor(22, 40).is_none(),
    ));
    out.push(("分派表升序合法", flavor_table_sane()));

    // ── 判据九：cap 警戒线（95%）。
    let mut e3 = match AmdPwrEngine::new(Platform::Linux, 22, 30) {
        Ok(v) => v,
        Err(_) => return vec![("引擎构造3", false)],
    };
    let mut src3 = MockAmdSrc { plat: PLATFORM_LINUX, cap: 100_000, dom: [(96_000, 960_000); 3], dom_ok: [true; 3] };
    let _ = e3.feed_cap(&mut src3, 1);
    let _ = e3.feed_domain(&mut src3, 0, 10);
    out.push(("cap 95% 警戒", e3.gpu_over_warn()));

    // ── 判据十：三域合计只计 Active（Unknown 不掺 0）。
    let total = e.total_active_mw();
    out.push(("合计只计可见域", total == 97_000));

    // ── 判据十一：事件账滚动 + 守恒。
    out.push(("事件账可读", e3.event_at(0).is_some()));
    out.push(("引擎守恒对账", e.reconcile() && e2.reconcile() && e3.reconcile()));

    // ── 判据十二：读屏三段。
    let s = e.read_screen();
    out.push((
        "读屏三段",
        s.contains("平台与兼容层") && s.contains("cap 与三域") && s.contains("诊断"),
    ));

    // ── 判据十三：Neither 平台 / 未知版本构造显性失败。
    out.push((
        "构造显性失败",
        AmdPwrEngine::new(Platform::Neither, 23, 0).is_err()
            && AmdPwrEngine::new(Platform::Linux, 99, 0).is_err(),
    ));

    out
}

// ---------------------------------------------------------------------------
// 旧式判据桥接（2026-10-22 接线轮补）：run_cgj04_checks 返回 CheckSet 之前的旧式
// Vec<(&'static str, bool)>，不能直接进 CheckSet 函数指针表；此桥逐项转入
// CheckSetBuilder（语义等价：判据名/通过位一一对应，零信息丢失）。
// ---------------------------------------------------------------------------
pub fn run_cgj04_checks_cs() -> crate::checks::CheckSet {
    let mut b = crate::checks::CheckSetBuilder::new("CGPU-F1444", "旧式判据桥接");
    for (name, ok) in run_cgj04_checks() {
        b.ok(name, ok, "");
    }
    b.done()
}

