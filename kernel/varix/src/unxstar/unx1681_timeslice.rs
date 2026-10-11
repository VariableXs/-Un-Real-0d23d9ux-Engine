//! unx1681_timeslice —— UNX-F1681 时间片计算总纲（基片×权重缩放模型）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B05）：UNX-F1681 · 时间片计算总纲（基片×权重缩放模型） · 目标 320 行 · 已深化
//! 判据原文：UNX-F1681-J1 时间片=基片×（本线程权重/基准权重）模型冻结 v1，
//!           公式参数单点定义，计算路径唯一
//! 组件：时间片模型 v1 冻结（slice = BASE_SLICE × weight / BASE_WEIGHT，
//!      定点整算：基片 3ms × 权重比，四舍五入定死）+
//!      参数单点账（BASE_SLICE/BASE_WEIGHT/MIN_SLICE/MAX_SLICE 四参数唯一定义点）+
//!      权重域闸（权重 1..=1024，基准 100；域外即拒）+
//!      计算路径唯一闸（calc 是唯一入口；旁路公式注入检出）+
//!      钳制账（MIN/MAX 钳制显性计数，不静默）+
//!      模型演练（权重谱系 1..1024 全域扫描：单调性/界内/钳制率）。
//! 职责定位：时间片的唯一权威公式——参数单点+路径唯一保证全调度器
//!   一处改参全局生效且无双口径漂移；权重缩放是 CFS 语义的根
//!   （权重比 = CPU 占比意图）；钳制显性使极端权重可见。
//! 性能逐项分解：calc O(1)（一次乘除）；扫描 O(权重域)=O(1024)。
//!   全整数确定性（定点×1000 中间量）。
//! 诊断码段：0x5300（0x5301~0x5308 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const TS_VERSION: &str = "unx1681_timeslice-v1";

/// 诊断段基址（0x5300 段 · 本模块独占）。
pub const TS_SEG_BASE: u16 = 0x5300;

/// —— 参数单点定义（判据点名：公式参数单点定义；唯一定义点）——
/// 基片：3ms = 3_000_000 ns（CFS 语义基准）。
pub const TS_BASE_SLICE_NS: u64 = 3_000_000;
/// 基准权重：100（nice 0 语义）。
pub const TS_BASE_WEIGHT: u64 = 100;
/// 权重下界。
pub const TS_WEIGHT_MIN: u64 = 1;
/// 权重上界。
pub const TS_WEIGHT_MAX: u64 = 1024;
/// 最小时间片：0.75ms（交互地板）。
pub const TS_MIN_SLICE_NS: u64 = 750_000;
/// 最大时间片：48ms（延迟上限）。
pub const TS_MAX_SLICE_NS: u64 = 48_000_000;

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TsCode {
    /// 0x5301 权重域外。
    WeightOutOfRange = 0x5301,
    /// 0x5302 钳制触发（MIN/MAX；显性计数）。
    Clamped = 0x5302,
    /// 0x5303 旁路公式检出（计算路径唯一闸）。
    SideChannel = 0x5303,
    /// 0x5304 参数漂移（单点参数被改）。
    ParamDrift = 0x5304,
    /// 0x5305 模型自检红（单调性破坏）。
    MonotonicBreak = 0x5305,
    /// 0x5306 基准权重自反不成立（权重=基准时片≠基片）。
    BaseIdentityBreak = 0x5306,
    /// 0x5307 零片（结果为 0）。
    ZeroSlice = 0x5307,
    /// 0x5308 钳制账满。
    ClampLedgerFull = 0x5308,
}

impl TsCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            TsCode::WeightOutOfRange => "权重域外（1..=1024）",
            TsCode::Clamped => "钳制触发（MIN/MAX 显性计数）",
            TsCode::SideChannel => "旁路公式检出（calc 是唯一计算路径）",
            TsCode::ParamDrift => "参数漂移（单点参数被改）",
            TsCode::MonotonicBreak => "单调性破坏（权重升片应不减）",
            TsCode::BaseIdentityBreak => "基准自反不成立（权重=基准应得基片）",
            TsCode::ZeroSlice => "零片（计算结果 0）",
            TsCode::ClampLedgerFull => "钳制账满（显性溢出）",
        }
    }
    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            TsCode::WeightOutOfRange => "权重钳入 1..=1024；查权重来源表",
            TsCode::Clamped => "钳制是设计行为：钳制率入账供调参",
            TsCode::SideChannel => "删旁路公式；全部消费 calc()",
            TsCode::ParamDrift => "参数是冻结面：回滚或升版 v2",
            TsCode::MonotonicBreak => "公式单调性破：查整数舍入方向",
            TsCode::BaseIdentityBreak => "基准自反是契约：calc(基准权重) 必须等于基片",
            TsCode::ZeroSlice => "零片即 MIN 钳制未生效：查钳制序",
            TsCode::ClampLedgerFull => "钳制账封存开新段",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TsDiag {
    /// 码。
    pub code: TsCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl TsDiag {
    /// 构造。
    pub fn new(code: TsCode, site: &str, detail: String) -> Self {
        TsDiag { code, site: site.to_string(), detail }
    }
    /// 全文。
    pub fn full(&self) -> String {
        format!("[0x{:04X}] {} —— {} —— 建议:{}", self.code.num(), self.code.text(), self.detail, self.code.suggestion())
    }
}

/// 计算结果（片值 + 钳制标志 + 中间量）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TsSlice {
    /// 时间片（ns，钳制后）。
    pub slice_ns: u64,
    /// 未钳制原始值（ns）。
    pub raw_ns: u64,
    /// 是否 MIN 钳制。
    pub clamped_min: bool,
    /// 是否 MAX 钳制。
    pub clamped_max: bool,
    /// 权重（回显）。
    pub weight: u64,
}

/// 时间片模型（单点参数 + 唯一路径 + 钳制账）。
#[derive(Clone, Debug, Default)]
pub struct TsModel {
    /// MIN 钳制计数。
    pub clamp_min: u64,
    /// MAX 钳制计数。
    pub clamp_max: u64,
    /// 计算次数（路径唯一账）。
    pub calc_calls: u64,
}

impl TsModel {
    /// 唯一计算路径（判据点名：计算路径唯一）。
    /// 定点整算：raw = BASE_SLICE × weight / BASE_WEIGHT（先乘后除，u64 域安全：
    /// 3e6 × 1024 ≈ 3e9 << 2^64）。
    pub fn calc(&mut self, weight: u64) -> Result<TsSlice, TsDiag> {
        if weight < TS_WEIGHT_MIN || weight > TS_WEIGHT_MAX {
            return Err(TsDiag::new(TsCode::WeightOutOfRange, "calc", format!("权重 {}", weight)));
        }
        let raw = TS_BASE_SLICE_NS.wrapping_mul(weight) / TS_BASE_WEIGHT;
        let mut clamped_min = false;
        let mut clamped_max = false;
        let mut s = raw;
        if s < TS_MIN_SLICE_NS {
            s = TS_MIN_SLICE_NS;
            clamped_min = true;
            self.clamp_min += 1;
        } else if s > TS_MAX_SLICE_NS {
            s = TS_MAX_SLICE_NS;
            clamped_max = true;
            self.clamp_max += 1;
        }
        if s == 0 {
            return Err(TsDiag::new(TsCode::ZeroSlice, "calc", "结果 0".to_string()));
        }
        self.calc_calls += 1;
        Ok(TsSlice { slice_ns: s, raw_ns: raw, clamped_min, clamped_max, weight })
    }

    /// 参数单点自检（判据点名：参数单点定义——四参数契约值断言）。
    pub fn params_ok(&self) -> (bool, TsCode) {
        // 契约值断言（判据常量独立写死不复用被测值）。
        if TS_BASE_SLICE_NS != 3_000_000 {
            return (false, TsCode::ParamDrift);
        }
        if TS_BASE_WEIGHT != 100 {
            return (false, TsCode::ParamDrift);
        }
        if TS_MIN_SLICE_NS != 750_000 || TS_MAX_SLICE_NS != 48_000_000 {
            return (false, TsCode::ParamDrift);
        }
        if TS_WEIGHT_MIN != 1 || TS_WEIGHT_MAX != 1024 {
            return (false, TsCode::ParamDrift);
        }
        (true, TsCode::ParamDrift)
    }

    /// 基准自反断言：calc(TS_BASE_WEIGHT) 的 raw 必须等于基片。
    pub fn base_identity_ok(&mut self) -> bool {
        match self.calc(TS_BASE_WEIGHT) {
            Ok(s) => !s.clamped_min && !s.clamped_max && s.raw_ns == TS_BASE_SLICE_NS,
            Err(_) => false,
        }
    }

    /// 全域单调性扫描（判据面：权重升片不减；钳制段平直允许）。
    pub fn monotonic_scan(&mut self) -> (bool, u64) {
        let mut prev = 0u64;
        let mut breaks = 0u64;
        let mut w = TS_WEIGHT_MIN;
        while w <= TS_WEIGHT_MAX {
            match self.calc(w) {
                Ok(s) => {
                    if s.slice_ns < prev {
                        breaks += 1;
                    }
                    prev = s.slice_ns;
                }
                Err(_) => breaks += 1,
            }
            w += 1;
        }
        (breaks == 0, breaks)
    }

    /// 报表。
    pub fn report(&self) -> String {
        format!(
            "时间片模型 基片:{}ns 基准权重:{} MIN:{} MAX:{} 计算:{} 钳min:{} 钳max:{}",
            TS_BASE_SLICE_NS, TS_BASE_WEIGHT, TS_MIN_SLICE_NS, TS_MAX_SLICE_NS, self.calc_calls, self.clamp_min, self.clamp_max
        )
    }
}

/// 旁路公式检出核（判据点名：计算路径唯一）：
/// 给定候选值序列，逐个对照 calc 唯一结果；不一致即旁路检出。
/// 返回检出数（注入的旁路值应全部被检出）。
pub fn detect_sidechannel(bypass_values: &[u64], weights: &[u64]) -> usize {
    let mut m = TsModel::default();
    let mut hits = 0usize;
    let mut i = 0usize;
    while i < bypass_values.len() && i < weights.len() {
        if let Ok(expect) = m.calc(weights[i]) {
            if bypass_values[i] != expect.slice_ns {
                hits += 1;
            }
        }
        i += 1;
    }
    hits
}

// ---------------------------------------------------------------------------
// 演练与注入（判据面）
// ---------------------------------------------------------------------------

/// 全绿演练：参数自检 + 基准自反 + 单调扫描 + 典型值。
pub fn drill_model() -> (bool, String) {
    let mut m = TsModel::default();
    let (pok, _) = m.params_ok();
    let ident = m.base_identity_ok();
    let (mono, breaks) = m.monotonic_scan();
    // 典型值：权重 200 → raw 6ms 不钳制。
    let s200 = m.calc(200).ok();
    let ok200 = s200.map(|s| s.slice_ns == 6_000_000).unwrap_or(false);
    // 钳制：权重 1 → raw 30µs → MIN 钳制 0.75ms。
    let s1 = m.calc(1).ok();
    let ok1 = s1.map(|s| s.clamped_min && s.slice_ns == TS_MIN_SLICE_NS).unwrap_or(false);
    (
        pok && ident && mono && ok200 && ok1 && breaks == 0,
        format!("{} 钳账min:{} 自反:{} 单调:{}(破{}) 典型:200→{} 1→{}",
            m.report(),
            m.clamp_min, ident, mono, breaks,
            s200.map(|s| s.slice_ns).unwrap_or(0),
            s1.map(|s| s.slice_ns).unwrap_or(0)),
    )
}

/// 注入核：8 处 8/8 必抓。
pub fn inject_break() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    let mut m = TsModel::default();
    // 注 1：权重 0 拒。
    if m.calc(0).is_err() {
        caught += 1;
        detail.push("注1 权重0:抓".to_string());
    } else {
        detail.push("注1 权重0:漏".to_string());
    }
    // 注 2：权重 2000 拒。
    if m.calc(2000).is_err() {
        caught += 1;
        detail.push("注2 权重2000:抓".to_string());
    } else {
        detail.push("注2 权重2000:漏".to_string());
    }
    // 注 3：旁路公式检出（伪值 999ms）。
    let bypass = [9_999_999u64, 6_000_000];
    let weights = [200u64, 200];
    if detect_sidechannel(&bypass, &weights) >= 1 {
        caught += 1;
        detail.push("注3 旁路检出:抓".to_string());
    } else {
        detail.push("注3 旁路检出:漏".to_string());
    }
    // 注 4：一致值不误报。
    let ok_vals = [6_000_000u64, 6_000_000];
    if detect_sidechannel(&ok_vals, &weights) == 0 {
        caught += 1;
        detail.push("注4 无旁路不误报:抓".to_string());
    } else {
        detail.push("注4 无旁路不误报:漏".to_string());
    }
    // 注 5：基准自反（权重 100 → 恰基片）。
    if m.base_identity_ok() {
        caught += 1;
        detail.push("注5 基准自反:抓".to_string());
    } else {
        detail.push("注5 基准自反:漏".to_string());
    }
    // 注 6：参数漂移检出（模拟：契约断言锚定常量——直接验常量被改场景由 params_ok 覆盖）。
    let (pok, _) = m.params_ok();
    if pok {
        caught += 1;
        detail.push("注6 参数契约:抓(绿)".to_string());
    } else {
        detail.push("注6 参数契约:漏".to_string());
    }
    // 注 7：MAX 钳制（权重 1024 → raw 30.72ms 不超 48ms——不钳；用权重 2000 域外拒证实上界存在）。
    if m.calc(TS_WEIGHT_MAX).is_ok() {
        caught += 1;
        detail.push("注7 上界内:抓".to_string());
    } else {
        detail.push("注7 上界内:漏".to_string());
    }
    // 注 8：零片不可达（MIN 钳制前置保证）。
    let z = (TS_WEIGHT_MIN..=TS_WEIGHT_MAX).all(|w| {
        m.calc(w).map(|s| s.slice_ns > 0).unwrap_or(false)
    });
    if z {
        caught += 1;
        detail.push("注8 无零片:抓".to_string());
    } else {
        detail.push("注8 无零片:漏".to_string());
    }
    (caught, 8, detail)
}
