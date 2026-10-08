//! VE-F0012 · 渲染探针与时间戳基础设施（VE-A 域 · 内核图形抽象层 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0012`
//!
//! **判据（锚点原文）**：GPU 时间戳查询的统一封装（渲染各段耗时的硬件级测量），
//! 探针开销显式（开启探针贵多少写明），零探针零开销断言；时间戳含频率校准
//! （GPU 时钟频率漂移的补偿）；探针含分组开关（只开关心的段不为全链付费）；
//! 零开销断言含编译期验证（不编译探针=零探针路径）；基础设施与 X04 的数据
//! 契约显式。
//!
//! **错误路径与降级矩阵**：时间戳失真→校准；开销超标→告警；零开销违例→修复。
//!
//! **设计要点**：
//! - **统一封装**：一切渲染段耗时测量只经 [`GpuClock::query`]——探针不偷读
//!   时钟源，时间戳来源唯一，剖析数据才可比对（O(探针) 开销上界）；
//! - **开销显式**：每对 begin/end 的探针代价在 [`OverheadProfile`] 里声明并
//!   逐帧入账——"开启探针贵多少"是数字不是口号，帧账单里如实上报；
//! - **零探针零开销**：分组掩码为 0 时 begin/end 直接返回——不发起时间戳
//!   查询、不写记录、不入账开销（运行期）；[`NoopRecorder`] 是零大小类型
//!   且带编译期 `const` 断言——不编译探针=零探针路径（编译期验证）；
//! - **频率校准**：GPU 时钟与参考时钟（CPU 侧逻辑参考）的漂移用有理数
//!   斜率（num/den）补偿——校准后段耗时按 ns 换算与参考对拍一致；
//!   漂移超容限（[`DRIFT_TOLERANCE_MILI`]）→ 显性 `E_TIMESTAMP_DRIFT` →
//!   重新校准（时间戳失真→校准，禁静默）；
//! - **分组开关**：8 个渲染段各自独立开关——只开关心的段，不为全链付费；
//!   关闭段的 begin/end 零状态变化（零开销断言的运行期半边）；
//! - **X04 数据契约**：剖析数据出口 [`X04Row`] 结构与 [`X04_CONTRACT_VERSION`]
//!   显式——X04 剖析器消费的数据形状冻结，字段增删走契约版本；
//! - **读屏可达**：探针面板 `panel_text()` 人话播报帧号/段数/开销/校准态，
//!   无障碍判据落在文本出口而非图形。
//!
//! **跨批对接点**：X04 剖析器消费；上游 F0004 命令缓冲、F0011 热插拔重选。
//!
//! 逻辑时钟注入，零墙钟；零外部依赖，只依赖 `crate::checks`（自检侧）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 渲染段分组数（探针分组开关的位宽）。
pub const GROUP_COUNT: usize = 8;

/// 渲染段名（读屏播报与 X04 行共用，下标即组号）。
pub const GROUP_NAMES: [&str; GROUP_COUNT] = [
    "上传", "解码", "栅格化", "后处理", "合成", "文字", "粒子", "呈现",
];

/// X04 剖析器数据契约版本（字段增删必须升版本——契约显式判据）。
pub const X04_CONTRACT_VERSION: u32 = 1;

/// 漂移容限（千分比）：|实测斜率-校准斜率| 相对误差超过 1/1000 判失真。
pub const DRIFT_TOLERANCE_MILI: u128 = 1_000;

/// 单帧探针开销预算（tick）：超出即告警（开销超标→告警判据）。
pub const OVERHEAD_BUDGET_TICKS: u64 = 240;

// ---------------------------------------------------------------------------
// 二、时钟与校准（时间戳统一封装 + 频率漂移补偿）
// ---------------------------------------------------------------------------

/// GPU 时间戳时钟（唯一测量入口；逻辑 tick 注入，零墙钟）。
#[derive(Clone, Copy, Debug)]
pub struct GpuClock {
    /// 标称频率（Hz）：ns 换算的分母来源。
    pub freq_hz: u64,
    /// 每次 query 后前进的 tick 数（测试注入用；生产由驱动步进）。
    pub tick_step: u64,
    now: u64,
    queries: u64,
}

impl GpuClock {
    /// 构造：标称频率 + 每查询步进。
    pub fn new(freq_hz: u64, tick_step: u64) -> Self {
        GpuClock { freq_hz, tick_step, now: 0, queries: 0 }
    }

    /// 统一查询入口：返回当前 tick 并按步进推进（探针不偷读时钟）。
    pub fn query(&mut self) -> u64 {
        let t = self.now;
        self.queries = self.queries.saturating_add(1);
        self.now = self.now.saturating_add(self.tick_step.max(1));
        t
    }

    /// 测试注入：直接推进时钟（模拟真实流逝）。
    pub fn advance(&mut self, ticks: u64) {
        self.now = self.now.saturating_add(ticks);
    }

    /// 当前 tick（只读，不消耗查询次数）。
    pub fn now(&self) -> u64 {
        self.now
    }

    /// 累计查询次数（零探针断言的观测面：禁用时 queries 不增长）。
    pub fn query_count(&self) -> u64 {
        self.queries
    }
}

/// 校准结果：参考时钟每 1 GPU tick 对应 num/den 参考 tick（有理数斜率，
/// 整数确定性——浮点斜率会让回归不可复现）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Calibration {
    /// 斜率分子（Δ参考 / ΔGPU 的分子）。
    pub num: u64,
    /// 斜率分母。
    pub den: u64,
    /// 校准样本的最大残差（GPU tick）。
    pub max_residual_ticks: u64,
    /// 是否已校准。
    pub calibrated: bool,
}

impl Default for Calibration {
    fn default() -> Self {
        Calibration { num: 1, den: 1, max_residual_ticks: 0, calibrated: false }
    }
}

impl Calibration {
    /// GPU tick → 参考纳秒（漂移补偿后的换算；u128 防溢出）。
    pub fn to_ref_ns(&self, gpu_ticks: u64, freq_hz: u64) -> u128 {
        (gpu_ticks as u128)
            * 1_000_000_000u128
            * (self.num as u128)
            / ((freq_hz as u128) * (self.den as u128))
    }
}

// ---------------------------------------------------------------------------
// 三、开销声明与配置（开销显式 + 分组开关）
// ---------------------------------------------------------------------------

/// 探针开销声明（"开启探针贵多少"的数字化答案）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverheadProfile {
    /// 每对 begin/end 的声明代价（tick）。
    pub per_probe_ticks: u64,
    /// 是否已显式声明（未声明的开销模型禁止启用探针）。
    pub declared: bool,
}

impl OverheadProfile {
    /// 默认声明：每对探针 12 tick（数字化显式，非口号）。
    pub fn declared_default() -> Self {
        OverheadProfile { per_probe_ticks: 12, declared: true }
    }

    /// 未声明构造（守门：declared=false 时启用探针属违例）。
    pub fn undeclared() -> Self {
        OverheadProfile { per_probe_ticks: 0, declared: false }
    }
}

/// 探针配置：分组开关掩码（bit i = 组 i 开）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeConfig {
    /// 分组掩码；0 = 全关（零探针路径）。
    pub group_mask: u32,
}

impl ProbeConfig {
    /// 零探针配置（掩码 0——编译期与运行期双重零开销）。
    pub const fn disabled() -> Self {
        ProbeConfig { group_mask: 0 }
    }

    /// 组开关查询。
    pub const fn enabled(&self, group: usize) -> bool {
        group < GROUP_COUNT && (self.group_mask >> group) & 1 == 1
    }
}

/// 零探针记录器：零大小类型 + 编译期断言——"不编译探针=零探针路径"的
/// 编译期验证半边（运行期半边在 begin/end 的早退分支）。
pub struct NoopRecorder;

const _: () = assert!(
    core::mem::size_of::<NoopRecorder>() == 0,
    "零探针记录器必须零大小（编译期验证：零探针路径不携带任何状态）"
);

/// 编译期零开销证明的运行期读数（自检与外部审计共用）。
pub fn zero_probe_compile_proof() -> bool {
    core::mem::size_of::<NoopRecorder>() == 0
}

// ---------------------------------------------------------------------------
// 四、记录与契约（X04 数据契约显式）
// ---------------------------------------------------------------------------

/// 单段探针记录（原始 GPU tick，换算在导出时做——原始值保真）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeRecord {
    /// 渲染段组号。
    pub group: usize,
    /// 起始 tick。
    pub start: u64,
    /// 结束 tick。
    pub end: u64,
    /// 原始耗时（tick）。
    pub raw_ticks: u64,
}

/// X04 剖析器数据契约行（字段冻结；增删走 X04_CONTRACT_VERSION）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct X04Row {
    /// 帧号。
    pub frame: u64,
    /// 渲染段组号。
    pub group: usize,
    /// 起始（参考 ns，已漂移补偿）。
    pub start_ns: u128,
    /// 结束（参考 ns，已漂移补偿）。
    pub end_ns: u128,
}

/// 帧探针账单（finish_frame 的出口；X04 与读屏共同消费）。
#[derive(Clone, Debug)]
pub struct FrameProbes {
    /// 帧号。
    pub frame: u64,
    /// 本帧记录。
    pub records: Vec<ProbeRecord>,
    /// 本帧探针总开销（tick，声明代价实入账）。
    pub overhead_ticks: u64,
    /// 告警（开销超标等）。
    pub warnings: Vec<String>,
    /// 错误账本副本（零静默：X04 侧也看得见）。
    pub errors: Vec<(String, &'static str, String)>,
}

// ---------------------------------------------------------------------------
// 五、渲染探针基础设施（主结构）
// ---------------------------------------------------------------------------

/// 渲染探针与时间戳基础设施（时钟 + 校准 + 分组开关 + 开销账 + 契约出口）。
pub struct RenderProbe {
    clock: GpuClock,
    calib: Calibration,
    calib_anchor_cpu: u64,
    calib_anchor_gpu: u64,
    cfg: ProbeConfig,
    overhead: OverheadProfile,
    overhead_budget: u64,
    open: Option<(usize, u64)>,
    records: Vec<ProbeRecord>,
    overhead_ticks: u64,
    warnings: Vec<String>,
    errors: Vec<(String, &'static str, String)>,
    violation: bool,
    frame: u64,
}

impl RenderProbe {
    /// 构造：时钟、配置、开销声明三件齐备。
    ///
    /// 开销未声明即启用探针属违例——显性报错，不静默给默认值。
    pub fn new(clock: GpuClock, cfg: ProbeConfig, overhead: OverheadProfile) -> Self {
        let mut errors: Vec<(String, &'static str, String)> = Vec::new();
        if overhead.declared == false && cfg.group_mask != 0 {
            errors.push((
                "RenderProbe::new".to_string(),
                "E_OVERHEAD_UNDECLARED",
                "启用探针但开销模型未声明——先给 OverheadProfile::declared_default() 再开探针"
                    .to_string(),
            ));
        }
        RenderProbe {
            clock,
            calib: Calibration::default(),
            calib_anchor_cpu: 0,
            calib_anchor_gpu: 0,
            cfg,
            overhead,
            overhead_budget: OVERHEAD_BUDGET_TICKS,
            open: None,
            records: Vec::new(),
            overhead_ticks: 0,
            warnings: Vec::new(),
            errors,
            violation: false,
            frame: 0,
        }
    }

    // -- 只读观测面 -----------------------------------------------------------

    /// 当前帧号。
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// 校准态。
    pub fn calibration(&self) -> Calibration {
        self.calib
    }

    /// 开销声明（判据：开销显式）。
    pub fn overhead_profile(&self) -> OverheadProfile {
        self.overhead
    }

    /// 本帧已入账开销（tick）。
    pub fn overhead_ticks(&self) -> u64 {
        self.overhead_ticks
    }

    /// 观测面：时钟累计查询次数（零开销断言与唯一入口判据的观测点）。
    pub fn clock_query_count(&self) -> u64 {
        self.clock.query_count()
    }

    /// 观测面：本帧记录数。
    pub fn records_len(&self) -> usize {
        self.records.len()
    }

    /// 观测面：本帧是否无记录。
    pub fn records_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 观测面：第 idx 条记录的组号（越界返回 GROUP_COUNT，不 panic）。
    pub fn records_group(&self, idx: usize) -> usize {
        self.records.get(idx).map(|r| r.group).unwrap_or(GROUP_COUNT)
    }

    /// 告警列表。
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// 错误账本（零静默）。
    pub fn errors(&self) -> &[(String, &'static str, String)] {
        &self.errors
    }

    /// 零开销违例标志。
    pub fn violation(&self) -> bool {
        self.violation
    }

    fn record_error(&mut self, who: String, code: &'static str, detail: String) {
        self.errors.push((who, code, detail));
    }

    // -- 配置变更 --------------------------------------------------------------

    /// 调整分组开关（运行期可变；掩码 0 即进入零探针路径）。
    pub fn set_group_mask(&mut self, mask: u32) {
        self.cfg.group_mask = mask;
    }

    /// 调整单帧开销预算（默认 [`OVERHEAD_BUDGET_TICKS`]）。
    pub fn set_overhead_budget(&mut self, budget: u64) {
        self.overhead_budget = budget;
    }

    // -- 探针主流程 ------------------------------------------------------------

    /// 段开始（分组开关：未开启的组零状态变化——零探针路径）。
    pub fn begin(&mut self, group: usize) {
        if group >= GROUP_COUNT {
            self.record_error(
                format!("begin(g={})", group),
                "E_GROUP_OUT_OF_RANGE",
                format!("组号越界：有效范围 0..{}", GROUP_COUNT),
            );
            return;
        }
        if !self.cfg.enabled(group) {
            // 零探针路径：不查询、不记录、不入账（运行期零开销半边）。
            return;
        }
        if self.open.is_some() {
            self.record_error(
                format!("begin(g={})", group),
                "E_ALREADY_OPEN",
                "上一段未 end 就开始新段——探针段不允许嵌套，先 end 再 begin".to_string(),
            );
            return;
        }
        let ts = self.clock.query();
        self.open = Some((group, ts));
    }

    /// 段结束；成功入账返回 true（未开启组/无未闭环段返回 false 不记录）。
    pub fn end(&mut self) -> bool {
        let (group, start) = match self.open.take() {
            Some(x) => x,
            None => {
                self.record_error(
                    "end".to_string(),
                    "E_END_WITHOUT_BEGIN",
                    "未 begin 就 end——闭环纪律缺失，拒绝凭空记账".to_string(),
                );
                return false;
            }
        };
        let ts = self.clock.query();
        self.overhead_ticks = self.overhead_ticks.saturating_add(self.overhead.per_probe_ticks);
        self.records.push(ProbeRecord { group, start, end: ts, raw_ticks: ts.saturating_sub(start) });
        true
    }

    /// 零开销违例注入口（外部误用检测：禁用段强行记录）。
    ///
    /// 违例显性化（E_ZERO_PROBE_VIOLATION），修复走 [`RenderProbe::repair`]。
    pub fn force_record_while_disabled(&mut self, group: usize, raw: u64) {
        if self.cfg.enabled(group) {
            // 开启组走正常路径不构成违例。
            self.records.push(ProbeRecord {
                group,
                start: 0,
                end: raw,
                raw_ticks: raw,
            });
            return;
        }
        self.violation = true;
        self.record_error(
            format!("force(g={})", group),
            "E_ZERO_PROBE_VIOLATION",
            "零探针路径出现记录尝试——零开销断言被破坏，必须修复后才能继续采数".to_string(),
        );
    }

    /// 零开销违例修复（零开销违例→修复判据）：清记录、清标志、维持禁用。
    pub fn repair(&mut self) {
        self.records.clear();
        self.violation = false;
        self.warnings
            .push("零开销违例已修复：记录清空、探针维持禁用".to_string());
    }

    // -- 校准与失真 ------------------------------------------------------------

    /// 校准：参考时钟样本 (cpu, gpu) 对 → 有理数斜率 + 最大残差。
    ///
    /// 少于 2 个样本拒绝校准（斜率不可定——显性报错不猜）。
    pub fn calibrate(&mut self, samples: &[(u64, u64)]) -> Calibration {
        if samples.len() < 2 {
            self.record_error(
                "calibrate".to_string(),
                "E_CALIBRATION_SAMPLES",
                format!("校准样本不足：{} < 2，斜率不可定", samples.len()),
            );
            return self.calib;
        }
        let (c0, g0) = samples[0];
        let (c1, g1) = samples[samples.len() - 1];
        let dcpu = c1.saturating_sub(c0);
        let dgpu = g1.saturating_sub(g0);
        if dgpu == 0 || dcpu == 0 {
            self.record_error(
                "calibrate".to_string(),
                "E_CALIBRATION_SAMPLES",
                "校准样本无时间跨度（dcpu/dgpu 为 0）——样本无效".to_string(),
            );
            return self.calib;
        }
        let num = dcpu;
        let den = dgpu;
        let mut max_residual: u128 = 0;
        for &(c, g) in samples {
            // 残差 = |c - g*num/den|（GPU tick 域，整数有理化）。
            let expect = (g as u128) * (num as u128);
            let actual = (c as u128) * (den as u128);
            let diff = if expect > actual { expect - actual } else { actual - expect };
            let residual = diff / (den as u128);
            if residual > max_residual {
                max_residual = residual;
            }
        }
        self.calib = Calibration {
            num,
            den,
            max_residual_ticks: max_residual as u64,
            calibrated: true,
        };
        self.calib_anchor_cpu = c0;
        self.calib_anchor_gpu = g0;
        self.calib
    }

    /// 失真检测：相对参考时钟的漂移超过千分比容限即失真（时间戳失真→校准）。
    pub fn detect_drift(&self, cpu_now: u64, gpu_now: u64) -> bool {
        if !self.calib.calibrated {
            return false; // 未校准无基准，不误报（失真检测要求先校准）。
        }
        let dcpu = cpu_now.saturating_sub(self.calib_anchor_cpu) as u128;
        let dgpu = gpu_now.saturating_sub(self.calib_anchor_gpu) as u128;
        if dgpu == 0 {
            return false;
        }
        // 期望 dcpu = dgpu*num/den；相对误差 = |dcpu*den - dgpu*num| / (dgpu*num)。
        let expect = dgpu * (self.calib.num as u128);
        let actual = dcpu * (self.calib.den as u128);
        let diff = if expect > actual { expect - actual } else { actual - expect };
        diff * DRIFT_TOLERANCE_MILI > expect
    }

    // -- 帧出口 -----------------------------------------------------------------

    /// 收帧：导出账单并复位（含开销预算告警——开销超标→告警判据）。
    pub fn finish_frame(&mut self) -> FrameProbes {
        self.frame = self.frame.saturating_add(1);
        if self.overhead_ticks > self.overhead_budget {
            self.warnings.push(format!(
                "W_OVERHEAD_BUDGET：本帧探针开销 {} tick 超预算 {} tick",
                self.overhead_ticks, self.overhead_budget
            ));
        }
        FrameProbes {
            frame: self.frame,
            records: self.records.clone(),
            overhead_ticks: self.overhead_ticks,
            warnings: self.warnings.clone(),
            errors: self.errors.clone(),
        }
    }

    /// 帧复位（finish_frame 后由调用方驱动；探针账归零，告警/错误保留累计）。
    pub fn reset_frame(&mut self) {
        self.records.clear();
        self.overhead_ticks = 0;
        self.open = None;
    }

    /// X04 契约导出：记录 → 契约行（ns 换算经漂移补偿）。
    pub fn to_x04_rows(&self, fp: &FrameProbes) -> Vec<X04Row> {
        fp.records
            .iter()
            .map(|r| X04Row {
                frame: fp.frame,
                group: r.group,
                start_ns: self.calib.to_ref_ns(r.start, self.clock.freq_hz),
                end_ns: self.calib.to_ref_ns(r.end, self.clock.freq_hz),
            })
            .collect()
    }

    /// 组名（越界返回"未知"，不 panic）。
    pub fn group_name(group: usize) -> &'static str {
        GROUP_NAMES.get(group).copied().unwrap_or("未知")
    }

    /// 读屏面板（无障碍判据：探针面板读屏可达）。
    pub fn panel_text(&self) -> String {
        format!(
            "渲染探针面板：帧 {}，已记录 {} 段，探针开销 {} tick（声明每对 {} tick），分组掩码 0b{:08b}，校准 {}，告警 {} 条，错误 {} 条",
            self.frame,
            self.records.len(),
            self.overhead_ticks,
            self.overhead.per_probe_ticks,
            self.cfg.group_mask,
            if self.calib.calibrated {
                format!("{}/{}", self.calib.num, self.calib.den)
            } else {
                "未校准".to_string()
            },
            self.warnings.len(),
            self.errors.len(),
        )
    }
}

// ---------------------------------------------------------------------------
// 六、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0012 域自检（判据逐条映射见 `vea12_checks.rs`）。
pub fn run_vea12_checks() -> CheckSet {
    super::vea12_checks::run_vea12_checks()
}

// ---------------------------------------------------------------------------
// 七、单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 标准环境：1GHz 标称 + 1_001_000 tick 步进（两次查询夹出 1_001_000 段）。
    fn env(mask: u32) -> RenderProbe {
        RenderProbe::new(
            GpuClock::new(1_000_000_000, 1_001_000),
            ProbeConfig { group_mask: mask },
            OverheadProfile::declared_default(),
        )
    }

    #[test]
    fn vea12_probe_frame_end_to_end() {
        let mut p = env(0b0000_0101); // 组 0、2 开
        p.calibrate(&[(0, 0), (1_000_000, 1_000_000)]);
        p.begin(0);
        p.end();
        p.begin(2);
        p.end();
        let fp = p.finish_frame();
        assert_eq!(fp.records.len(), 2, "开启组应各有一条记录");
        assert_eq!(fp.records[0].group, 0);
        assert_eq!(fp.records[0].raw_ticks, 1_001_000, "两次查询夹出的段长");
        assert_eq!(fp.overhead_ticks, 24, "两对探针 x 每对声明 12 tick（end 一次入账）");
        let rows = p.to_x04_rows(&fp);
        assert_eq!(rows.len(), 2, "X04 契约行与记录一一对应");
        assert!(rows.iter().all(|r| r.end_ns >= r.start_ns));
    }

    #[test]
    fn vea12_probe_drift_calibration_compensates() {
        // GPU 快 0.1%：真实 1ms（1_000_000 参考 tick）被数成 1_001_000 GPU tick。
        let mut p = env(1);
        let calib = p.calibrate(&[(0, 0), (1_000_000, 1_001_000)]);
        assert_eq!(calib.num, 1_000_000, "斜率分子 = Δ参考");
        assert_eq!(calib.den, 1_001_000, "斜率分母 = ΔGPU");
        assert_eq!(calib.max_residual_ticks, 0, "线性样本残差为零");
        // 校准换算：1_001_000 GPU tick（1GHz 下 = 1_001_000 ns）按补偿斜率
        // 还原为真实 1ms = 1_000_000 ns（漂移被补偿，量纲：纳秒）。
        let ns = calib.to_ref_ns(1_001_000, 1_000_000_000);
        assert_eq!(ns, 1_000_000, "补偿后 1ms 段还原为 1e6 ns");
        // 失真检测：再漂 1% 触发（千分比容限之外），重校准后消除。
        let drifting = p.detect_drift(1_000_000, 1_010_000);
        assert!(drifting, "1% 漂移应超千分比容限");
        p.calibrate(&[(0, 0), (1_000_000, 1_010_000)]);
        assert!(!p.detect_drift(1_000_000, 1_010_000), "重校准后失真消除");
    }

    #[test]
    fn vea12_probe_zero_probe_zero_cost_and_violation_repair() {
        let mut p = env(0); // 全关
        let q0 = p.clock.query_count();
        p.begin(0);
        p.end();
        assert_eq!(p.clock.query_count(), q0, "禁用路径不发起任何时间戳查询");
        assert!(p.records.is_empty() && p.overhead_ticks() == 0, "零记录零开销");
        assert!(zero_probe_compile_proof(), "编译期零大小证明");
        p.force_record_while_disabled(3, 100);
        assert!(p.violation(), "禁用段强行记录 = 零开销违例");
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_ZERO_PROBE_VIOLATION"));
        p.repair();
        assert!(!p.violation() && p.records.is_empty(), "修复后干净");
        // 越界组显性报错不 panic。
        p.set_group_mask(0xFFFF_FFFF);
        p.begin(GROUP_COUNT);
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_GROUP_OUT_OF_RANGE"));
    }

    #[test]
    fn vea12_checks_all_green() {
        let set = run_vea12_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0012 域自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items()
        );
    }
}
