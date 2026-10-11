//! cgm07_pacemodel — CGPU-F0486 呈现节奏模型（呈现时钟的三模式行为建模）。
//!
//! 判据原文（施工书锚点 CGPU-F0486）：节奏模型：呈现时钟建模（VSync/无同步/
//! 合成器三种模式的帧间隔行为差异）；节奏策略（各模式的间隔管理策略——撕裂
//! vs 延迟的权衡声明）；模式适配（全屏独占/无边框窗口的节奏差异适配——与
//! D02 抑制联动）；模式探测（当前呈现模式自动识别）。判据：三模式、权衡声
//! 明、适配、探测、判据。
//!
//! 职责定位：三模式帧间隔行为表+撕裂/延迟权衡声明+窗口形态适配矩阵+模式探
//! 测器。与 cgm02_display/cgm05_modes（显示模式管理）分工——彼管模式切换执
//! 行，此管节奏行为的建模与探测（D02 抑制联动为声明式引用）。行为表独立写
//! 死可对拍。诊断码段 0x45xx。

use alloc::vec::Vec;

/// 模块版本。
pub const PACE_MODEL_VERSION: &str = "M07-pacemodel-v1";

/// 呈现三模式闭集（判据"VSync/无同步/合成器"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PresentMode {
    /// VSync：间隔锁刷新周期，零撕裂，延迟 1~2 帧。
    VSync,
    /// 无同步：不等待即时呈现，撕裂自由，延迟最低。
    Unsynced,
    /// 合成器节拍：跟随桌面合成器节奏，自适应间隔，无撕裂。
    Composer,
}

impl PresentMode {
    pub const ALL: [PresentMode; 3] = [
        PresentMode::VSync,
        PresentMode::Unsynced,
        PresentMode::Composer,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            PresentMode::VSync => 0,
            PresentMode::Unsynced => 1,
            PresentMode::Composer => 2,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            PresentMode::VSync => "垂直同步",
            PresentMode::Unsynced => "无同步",
            PresentMode::Composer => "合成器节拍",
        }
    }

    /// 权衡声明——撕裂风险（0=无 3=高；判据"撕裂 vs 延迟的权衡声明"）。
    pub const fn tear_risk(self) -> u8 {
        match self {
            PresentMode::VSync => 0,
            PresentMode::Unsynced => 3,
            PresentMode::Composer => 0,
        }
    }

    /// 权衡声明——呈现延迟（0=最低 3=最高）。
    pub const fn latency_cost(self) -> u8 {
        match self {
            PresentMode::VSync => 2,
            PresentMode::Unsynced => 0,
            PresentMode::Composer => 1,
        }
    }

    /// 权衡声明——间隔抖动（0=恒定 3=自由）。
    pub const fn jitter_cost(self) -> u8 {
        match self {
            PresentMode::VSync => 0,
            PresentMode::Unsynced => 2,
            PresentMode::Composer => 1,
        }
    }

    /// 间隔管理策略（判据"各模式的间隔管理策略"——策略文本闭集）。
    pub const fn pace_policy(self) -> &'static str {
        match self {
            PresentMode::VSync => "间隔锁刷新周期：等垂直空区，错过等下一周期（延迟阶梯化）",
            PresentMode::Unsynced => "间隔自由：渲染完即提交，不等待（撕裂换低延迟）",
            PresentMode::Composer => "间隔随合成器拍：自适应节拍，合成器何时取何时呈现",
        }
    }
}

/// 窗口形态闭集（判据"全屏独占/无边框窗口的节奏差异适配"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowMode {
    /// 全屏独占（可绕过合成器直通）。
    FullscreenExclusive,
    /// 无边框窗口（必经合成器）。
    Borderless,
    /// 窗口化（必经合成器）。
    Windowed,
}

impl WindowMode {
    pub const fn zh(self) -> &'static str {
        match self {
            WindowMode::FullscreenExclusive => "全屏独占",
            WindowMode::Borderless => "无边框窗口",
            WindowMode::Windowed => "窗口化",
        }
    }

    /// 适配矩阵：该窗口形态下允许的呈现模式（判据"模式适配"——
    /// 全屏独占三模式皆可；无边框/窗口化必经合成器，只允许 Composer
    /// 与 Unsynced（合成器直通被绕过的 VSync 语义在窗口形态下不成立））。
    pub const fn allows(self, m: PresentMode) -> bool {
        match self {
            WindowMode::FullscreenExclusive => true,
            WindowMode::Borderless | WindowMode::Windowed => {
                !matches!(m, PresentMode::VSync)
            }
        }
    }

    /// 适配说明（D02 抑制联动声明：窗口形态下 VSync 语义由合成器抑制接管）。
    pub const fn adapt_note(self) -> &'static str {
        match self {
            WindowMode::FullscreenExclusive => "全屏独占：三模式直通，D02 抑制不介入",
            WindowMode::Borderless => "无边框：必经合成器，VSync 语义由 D02 抑制联动接管",
            WindowMode::Windowed => "窗口化：必经合成器，VSync 语义由 D02 抑制联动接管",
        }
    }
}

/// 诊断码（0x45xx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x4501 模式闭集外。
    PmUnknown,
    /// 0x4502 模式×窗口组合不支持（适配矩阵拒绝）。
    PmComboUnsupported,
    /// 0x4503 探测信号歧义（两模式同样吻合——拒绝武断结论）。
    PmProbeAmbiguous,
    /// 0x4504 帧间隔抖动超模式声明上界（行为漂移显性）。
    PmJitterOver,
    /// 0x4505 帧间隔样本非法（0µs）。
    PmGapZero,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::PmUnknown => "呈现模式闭集外：只认 垂直同步/无同步/合成器节拍",
            XcCode::PmComboUnsupported => "模式×窗口组合不支持：适配矩阵拒绝（见 adapt_note）",
            XcCode::PmProbeAmbiguous => "探测信号歧义：多模式同样吻合，需更多帧样本",
            XcCode::PmJitterOver => "帧间隔抖动超该模式声明上界：呈现行为漂移显性立案",
            XcCode::PmGapZero => "帧间隔样本为 0：非法样本",
        }
    }
}

/// 探测信号集（判据"当前呈现模式自动识别"的输入）。含 Vec 不 impl Copy。
#[derive(Clone, Debug)]
pub struct ProbeSignals {
    /// 观测帧间隔集合（µs）。
    pub gaps_us: Vec<u32>,
    /// 刷新周期（µs，来自显示模式——60Hz=16667）。
    pub refresh_us: u32,
    /// 合成器拍存在（桌面合成器活跃标志）。
    pub composer_active: bool,
}

impl ProbeSignals {
    /// 间隔抖动（max-min，µs）。
    pub fn jitter_us(&self) -> u32 {
        let mut lo = u32::MAX;
        let mut hi = 0u32;
        for &g in &self.gaps_us {
            if g < lo {
                lo = g;
            }
            if g > hi {
                hi = g;
            }
        }
        if lo == u32::MAX {
            0
        } else {
            hi.saturating_sub(lo)
        }
    }

    /// 与刷新周期的锁相度（ppm）：间隔中位数与周期的偏差。
    pub fn lock_ppm(&self) -> u64 {
        if self.gaps_us.is_empty() || self.refresh_us == 0 {
            return 1_000_000;
        }
        let mut med = {
            let mut v = self.gaps_us.clone();
            v.sort_unstable();
            v[v.len() / 2] as u64
        };
        let r = self.refresh_us as u64;
        if med == 0 {
            med = 1;
        }
        let d = if med > r { med - r } else { r - med };
        d * 1_000_000 / r
    }
}

/// 模式探测器（判据"模式探测"本体——证据链判定，歧义显性）。
pub struct ModeProbe;

impl ModeProbe {
    /// 证据链判定：
    /// ① 间隔锁相刷新周期（偏差 ≤10%）且抖动 ≤1ms → VSync 或 Composer；
    ///    再按合成器活跃标志二分（活跃=Composer，否则 VSync）。
    /// ② 间隔抖动大（>2ms）且不锁相 → Unsynced。
    /// ③ 其余 → 歧义（显性拒绝武断结论）。
    pub fn detect(sig: &ProbeSignals) -> Result<PresentMode, XcCode> {
        if sig.gaps_us.is_empty() {
            return Err(XcCode::PmProbeAmbiguous);
        }
        for &g in &sig.gaps_us {
            if g == 0 {
                return Err(XcCode::PmGapZero);
            }
        }
        let lock = sig.lock_ppm() <= 100_000; // 偏差 ≤10%
        let jitter = sig.jitter_us();
        if lock && jitter <= 1_000 {
            if sig.composer_active {
                return Ok(PresentMode::Composer);
            }
            return Ok(PresentMode::VSync);
        }
        if jitter > 2_000 && !lock {
            return Ok(PresentMode::Unsynced);
        }
        Err(XcCode::PmProbeAmbiguous)
    }
}

/// 节奏核验器（判据"间隔管理策略"的运行期对拍：实测行为 vs 模式声明）。
pub struct PaceVerifier {
    /// 声明模式。
    mode: PresentMode,
    /// 刷新周期（µs）。
    refresh_us: u32,
    /// 观测间隔。
    gaps: Vec<u32>,
    /// 立案计数。
    pub cases: u32,
}

impl PaceVerifier {
    /// 构造（先过适配矩阵——组合不支持直接拒）。
    pub fn new(mode: PresentMode, win: WindowMode, refresh_us: u32) -> Result<Self, XcCode> {
        if !win.allows(mode) {
            return Err(XcCode::PmComboUnsupported);
        }
        Ok(PaceVerifier { mode, refresh_us, gaps: Vec::new(), cases: 0 })
    }

    /// 喂帧间隔。
    pub fn feed(&mut self, gap_us: u32) -> Result<(), XcCode> {
        if gap_us == 0 {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::PmGapZero);
        }
        self.gaps.push(gap_us);
        Ok(())
    }

    /// 行为对拍：抖动上界按模式声明（VSync ≤1ms / Composer ≤2ms /
    /// Unsynced 不判抖动判间隔非零——撕裂风险已在权衡声明中显性接受）。
    pub fn verify(&mut self) -> Result<(), XcCode> {
        if self.gaps.is_empty() {
            return Err(XcCode::PmProbeAmbiguous);
        }
        let jitter = {
            let mut lo = u32::MAX;
            let mut hi = 0u32;
            for &g in &self.gaps {
                if g < lo {
                    lo = g;
                }
                if g > hi {
                    hi = g;
                }
            }
            hi.saturating_sub(lo)
        };
        let bound = match self.mode {
            PresentMode::VSync => 1_000,
            PresentMode::Composer => 2_000,
            PresentMode::Unsynced => u32::MAX,
        };
        if jitter > bound {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::PmJitterOver);
        }
        Ok(())
    }

    /// 三模式权衡对照表（判据"权衡声明"的完整输出——消费方展示用）。
    pub fn tradeoff_table() -> [[u8; 3]; 3] {
        // [模式][撕裂, 延迟, 抖动]
        let mut t = [[0u8; 3]; 3];
        for (i, m) in PresentMode::ALL.iter().enumerate() {
            t[i][0] = m.tear_risk();
            t[i][1] = m.latency_cost();
            t[i][2] = m.jitter_cost();
        }
        t
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "节奏模型{} 模式{} 刷新{}µs 样本{} 立案{}",
            PACE_MODEL_VERSION,
            self.mode.zh(),
            self.refresh_us,
            self.gaps.len(),
            self.cases,
        )
    }
}

/// 多显示器节拍协商（深化：主从节拍——异刷新率多屏时以主屏刷新周期为锚，
/// 从屏按整数倍分频对齐；非整数倍率按最近整数倍取齐并留痕偏差）。
pub struct MultiMonPace;

impl MultiMonPace {
    /// 从屏分频系数（主屏周期 → 从屏呈现节奏）：从屏刷新率 ≤ 主屏时取
    /// 整数倍分频（主 120Hz 从 60Hz → 系数 2）；从屏更高速率时按 1（跟随）。
    pub const fn divider(master_hz: u32, slave_hz: u32) -> u32 {
        if master_hz == 0 || slave_hz == 0 {
            return 1;
        }
        if slave_hz >= master_hz {
            return 1;
        }
        let d = master_hz / slave_hz;
        if d == 0 {
            1
        } else {
            d
        }
    }

    /// 非整数倍偏差留痕（ppm）：slave_hz×divider 与 master_hz 的偏差——
    /// 偏差超 5% 建议换档（显示模式层消费）。
    pub const fn misalign_ppm(master_hz: u32, slave_hz: u32) -> u64 {
        let d = MultiMonPace::divider(master_hz, slave_hz) as u64;
        if d == 0 || master_hz == 0 {
            return 1_000_000;
        }
        let eff = slave_hz as u64 * d;
        let m = master_hz as u64;
        let diff = if eff > m { eff - m } else { m - eff };
        diff * 1_000_000 / m
    }
}

/// 间隔预测器（深化：EWMA 移位实现——α=1/4，预测下一帧间隔，
/// 供 VSync 模式的提交时机前移：预测间隔早于周期即提前提交）。
#[derive(Clone, Copy, Debug)]
pub struct GapPredictor {
    /// EWMA 状态（µs）。
    ewma_us: u64,
    /// 样本数。
    n: u32,
}

impl GapPredictor {
    pub const fn new() -> Self {
        GapPredictor { ewma_us: 0, n: 0 }
    }

    /// 喂间隔：ewma = ewma×3/4 + gap/4（全整数移位）。
    pub fn feed(&mut self, gap_us: u32) {
        let g = gap_us as u64;
        if self.n == 0 {
            self.ewma_us = g;
        } else {
            self.ewma_us = self.ewma_us.saturating_mul(3) / 4 + g / 4;
        }
        self.n = self.n.saturating_add(1);
    }

    /// 预测下一间隔（µs）。
    pub const fn predict_us(&self) -> u64 {
        self.ewma_us
    }

    /// 提交时机建议（VSync 用）：预测间隔比刷新周期早提前量以上 → 建议提前
    /// 提交（吃满周期），否则按期提交。
    pub const fn submit_early(&self, refresh_us: u32, lead_us: u64) -> bool {
        self.n >= 8 && self.ewma_us + lead_us < refresh_us as u64
    }
}

/// 自适应 VSync 迟滞策略（深化：帧率贴线时切无同步防倒退——迟滞带
/// 不对称：降档即时、升档需连续 8 帧好窗口证实——防抖动反复横跳）。
#[derive(Clone, Copy, Debug)]
pub struct AdaptiveVsync {
    /// 当前生效模式。
    pub cur: PresentMode,
    /// 连续好帧计数（升档证据）。
    good_run: u32,
    /// 升档门槛。
    pub good_need: u32,
}

/// 自适应判定线（µs）：贴线判据 = 1.25×12.5ms（P95 承诺的 125% 容带）。
pub const PROMISE_GAP_ADAPT_US: u32 = 15_625;

impl AdaptiveVsync {
    pub const fn new() -> Self {
        AdaptiveVsync { cur: PresentMode::VSync, good_run: 0, good_need: 8 }
    }

    /// 帧判定（gap 实测 vs 承诺）：超容带 → 立即降 Unsynced（降档即时）；
    /// 达标累计 ≥ 门槛 → 回升 VSync（升档迟滞）。
    pub fn on_frame(&mut self, gap_us: u32) -> PresentMode {
        if gap_us > PROMISE_GAP_ADAPT_US {
            self.good_run = 0;
            self.cur = PresentMode::Unsynced;
        } else {
            self.good_run = self.good_run.saturating_add(1);
            if self.good_run >= self.good_need {
                self.cur = PresentMode::VSync;
            }
        }
        self.cur
    }

    /// 好帧运行计数（对账用）。
    pub const fn good_run(&self) -> u32 {
        self.good_run
    }
}
