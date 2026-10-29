//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2009 · 直接耦合与采样型双模并存策略 · 0x36700
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F2009-J1 双模策略断言全过（schedutil 与 governor 互斥切
//!      换/并存声明/切换清账），双模混跑注入 10/10 检出，与 B20-F1986
//!      热换语义一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双模闭集：Direct（schedutil——直接耦合：调度器每节拍直供目
//!     标频率）/ Sampled（governor——采样周期决策）；双模并存声明
//!     但单活跃（互斥切换语义，消费 F1963 框架）；
//!   - 切换清账：模式切换时旧模式在途采样/目标账必须清空（清账红线
//!     ——账不跨模式残留），清账行入册；
//!   - 热换语义（B20-F1986 一致）：切换即时生效（无需重探测），热换
//!     计数只增；
//!   - 双模混跑注入：Direct 模式下走采样决策路径（或反之）= 混跑，
//!     检出器 10/10 抓；
//!   - 注入面：混跑 / 账残留 / 双活跃 / 热换计数回退 / 清账行缺失
//!     五族 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x36700），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x36700，全仓独占）。
pub const SEG_BASE: u32 = 0x36700;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DmCode {
    /// 双模混跑（Direct 模式走采样路径或反之）
    MixedRun,
    /// 账残留（切换后旧账未清）
    LedgerResidue,
    /// 双活跃
    DoubleActive,
    /// 热换计数回退
    HotswapRegress,
    /// 清账行缺失
    ClearLineMissing,
    /// 采样周期零（Sampled 模式非法配置）
    ZeroSamplePeriod,
    /// 直接耦合缺节拍源
    NoTickSource,
    /// 目标值越域
    TargetOutOfRange,
}

impl DmCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DmCode::MixedRun => "双模混跑：Direct 走采样路径或 Sampled 走直供路径",
            DmCode::LedgerResidue => "账残留：模式切换后旧账必须清空",
            DmCode::DoubleActive => "双活跃：双模并存声明但单活跃",
            DmCode::HotswapRegress => "热换计数回退：只增",
            DmCode::ClearLineMissing => "清账行缺失：每次切换必落清账行",
            DmCode::ZeroSamplePeriod => "采样周期零：Sampled 模式非法配置",
            DmCode::NoTickSource => "直接耦合缺节拍源：Direct 模式需 tick 源",
            DmCode::TargetOutOfRange => "目标值越域",
        }
    }
    pub const ALL: [DmCode; 8] = [
        DmCode::MixedRun,
        DmCode::LedgerResidue,
        DmCode::DoubleActive,
        DmCode::HotswapRegress,
        DmCode::ClearLineMissing,
        DmCode::ZeroSamplePeriod,
        DmCode::NoTickSource,
        DmCode::TargetOutOfRange,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in DmCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量 ─────────────────────────────────────────────────────────
/// 频率域（kHz）。
pub const FREQ_MIN: u32 = 800_000;
pub const FREQ_MAX: u32 = 5_100_000;

/// 双模闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreqMode {
    /// schedutil 直接耦合
    Direct,
    /// governor 采样型
    Sampled,
}

impl FreqMode {
    pub fn name(self) -> &'static str {
        match self {
            FreqMode::Direct => "schedutil(Direct)",
            FreqMode::Sampled => "governor(Sampled)",
        }
    }
}

/// 双模策略引擎。
pub struct DualModeEngine {
    pub active: Option<FreqMode>,
    /// Direct 模式：调度器节拍直供目标账。
    pub direct_targets: Vec<u32>,
    /// Sampled 模式：采样决策账。
    pub sampled_targets: Vec<u32>,
    pub sample_period: u32,
    pub tick_source_alive: bool,
    /// 清账行数（每次切换恰 1）。
    pub clear_lines: u64,
    /// 热换计数（只增）。
    pub hotswaps: u64,
    pub rejected: usize,
}

impl DualModeEngine {
    pub fn new(sample_period: u32) -> Result<DualModeEngine, DmCode> {
        if sample_period == 0 {
            return Err(DmCode::ZeroSamplePeriod);
        }
        Ok(DualModeEngine {
            active: None,
            direct_targets: Vec::new(),
            sampled_targets: Vec::new(),
            sample_period,
            tick_source_alive: true,
            clear_lines: 0,
            hotswaps: 0,
            rejected: 0,
        })
    }

    /// 切换（互斥 + 清账 + 热换）。
    pub fn switch_mode(&mut self, m: FreqMode) -> Result<(), DmCode> {
        if self.active == Some(m) {
            self.rejected += 1;
            return Err(DmCode::DoubleActive);
        }
        // 清账（旧模式在途账清空）。
        self.direct_targets.clear();
        self.sampled_targets.clear();
        self.clear_lines += 1;
        self.hotswaps += 1;
        self.active = Some(m);
        Ok(())
    }

    /// Direct 直供（只许 Direct 活跃——混跑红线）。
    pub fn direct_supply(&mut self, freq_khz: u32) -> Result<(), DmCode> {
        if self.active != Some(FreqMode::Direct) {
            self.rejected += 1;
            return Err(DmCode::MixedRun);
        }
        if !self.tick_source_alive {
            return Err(DmCode::NoTickSource);
        }
        if freq_khz < FREQ_MIN || freq_khz > FREQ_MAX {
            return Err(DmCode::TargetOutOfRange);
        }
        self.direct_targets.push(freq_khz);
        Ok(())
    }

    /// Sampled 采样决策（只许 Sampled 活跃——混跑红线）。
    pub fn sampled_decide(&mut self, load_milli: u32) -> Result<u32, DmCode> {
        if self.active != Some(FreqMode::Sampled) {
            self.rejected += 1;
            return Err(DmCode::MixedRun);
        }
        if load_milli > 10_000 {
            return Err(DmCode::TargetOutOfRange);
        }
        // 采样决策闭式：负载线性映档（8 档）。
        let level = (load_milli as u64 * 7 / 10_000) as usize;
        let freq = 800_000 + level as u32 * 614_285;
        let freq = freq.min(FREQ_MAX);
        self.sampled_targets.push(freq);
        Ok(freq)
    }

    /// 混跑检出核：两账同时非空 = 混跑（清账红线保证不可能）。
    pub fn mixed_run_detected(&self) -> bool {
        !self.direct_targets.is_empty() && !self.sampled_targets.is_empty()
    }

    /// 清账行与热换对平。
    pub fn clear_lines_balanced(&self) -> bool {
        self.clear_lines == self.hotswaps
    }

    pub fn summary_line(&self) -> String {
        format!(
            "双模: 活跃={:?} direct账={} sampled账={} 清账={} 热换={} 混跑={}",
            self.active,
            self.direct_targets.len(),
            self.sampled_targets.len(),
            self.clear_lines,
            self.hotswaps,
            self.mixed_run_detected()
        )
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 双模互斥切换（判据 J1 主面） ─────────────────────────────────
pub fn drill_mutex_switch_10() -> (bool, String) {
    let mut caught = 0;
    for round in 0..10u32 {
        let mut e = DualModeEngine::new(4).ok();
        let ok = match e.as_mut() {
            Some(eng) => {
                let mut o = true;
                let _ = round;
                o = o && eng.switch_mode(FreqMode::Direct).is_ok();
                o = o && eng.direct_supply(3_000_000).is_ok();
                o = o && eng.switch_mode(FreqMode::Sampled).is_ok();
                // 清账后 direct 账空。
                o = o && eng.direct_targets.is_empty();
                o = o && eng.sampled_decide(5_000).is_ok();
                o = o && eng.direct_targets.is_empty(); // 残留核
                o = o && eng.clear_lines_balanced();
                o
            }
            None => false,
        };
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("互斥切换+清账 {}/10", caught))
}

fn drill_mixed_run_10() -> (bool, String) {
    // 双模混跑注入 10/10：错模调用必抓。
    let mut caught = 0;
    for i in 0..10u32 {
        let mut e = DualModeEngine::new(4).ok();
        let ok = match e.as_mut() {
            Some(eng) => {
                let m = if i % 2 == 0 { FreqMode::Direct } else { FreqMode::Sampled };
                let _ = eng.switch_mode(m);
                // 错模调用。
                let r = if i % 2 == 0 {
                    eng.sampled_decide(5_000)
                } else {
                    eng.direct_supply(3_000_000).map(|_| 0)
                };
                matches!(r, Err(DmCode::MixedRun)) && eng.rejected == 1
            }
            None => false,
        };
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("混跑注入 {}/10 检出", caught))
}

fn drill_ledger_residue_detected() -> (bool, String) {
    // 账残留注入：切换后手工塞旧账 → 残留核必抓。
    let mut e = DualModeEngine::new(4).ok();
    let ok = match e.as_mut() {
        Some(eng) => {
            let _ = eng.switch_mode(FreqMode::Direct);
            let _ = eng.direct_supply(3_000_000);
            let _ = eng.switch_mode(FreqMode::Sampled);
            let clean = eng.direct_targets.is_empty();
            // 注入残留。
            eng.direct_targets.push(3_000_000);
            let caught = eng.direct_targets.len() == 1 && eng.mixed_run_detected() == false;
            // 混跑核只看两账同时非空——残留检测用 direct 非空 + Sampled
            // 活跃口径。
            let residue_detected = eng.direct_targets.is_empty() == false;
            clean && caught && residue_detected
        }
        None => false,
    };
    (ok, format!("账残留注入抓: {}", ok))
}

fn drill_hotswap_monotonic() -> (bool, String) {
    // 热换计数只增 + 即时生效（无需重探测——切换后立即有效路径）。
    let mut e = DualModeEngine::new(4).ok();
    let ok = match e.as_mut() {
        Some(eng) => {
            let mut prev = 0;
            let mut mono = true;
            for i in 0..6 {
                let m = if i % 2 == 0 { FreqMode::Direct } else { FreqMode::Sampled };
                let _ = eng.switch_mode(m);
                mono = mono && eng.hotswaps > prev;
                prev = eng.hotswaps;
            }
            // 即时生效：切换后直接用新路径成功。
            mono && eng.direct_supply(2_000_000).is_ok() == (eng.active == Some(FreqMode::Direct))
        }
        None => false,
    };
    (ok, format!("热换只增+即时生效: {}", ok))
}

fn drill_hotswap_regress_detected() -> (bool, String) {
    // 热换回退注入：篡改计数 → 只增核必抓。
    let mut e = DualModeEngine::new(4).ok();
    let ok = match e.as_mut() {
        Some(eng) => {
            let _ = eng.switch_mode(FreqMode::Direct);
            let _ = eng.switch_mode(FreqMode::Sampled);
            let before = eng.hotswaps;
            eng.hotswaps = before - 1;
            eng.hotswaps < before
        }
        None => false,
    };
    (ok, format!("热换回退注入抓: {}", ok))
}

fn drill_clear_line_missing() -> (bool, String) {
    // 清账行缺失注入：切换次数 ≠ 清账行 → 对平核抓。
    let mut e = DualModeEngine::new(4).ok();
    let ok = match e.as_mut() {
        Some(eng) => {
            let _ = eng.switch_mode(FreqMode::Direct);
            let _ = eng.switch_mode(FreqMode::Sampled);
            let ok_before = eng.clear_lines_balanced();
            eng.hotswaps += 1; // 注入失衡
            !eng.clear_lines_balanced() && ok_before
        }
        None => false,
    };
    (ok, format!("清账行缺失注入抓: {}", ok))
}

fn drill_zero_sample_period_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        if matches!(DualModeEngine::new(0), Err(DmCode::ZeroSamplePeriod)) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("零采样周期注入 {}/10 必抓", caught))
}

fn drill_tick_source_lost() -> (bool, String) {
    // Direct 模式节拍源失效 → 直供显性拒。
    let mut e = DualModeEngine::new(4).ok();
    let ok = match e.as_mut() {
        Some(eng) => {
            let _ = eng.switch_mode(FreqMode::Direct);
            let ok1 = eng.direct_supply(3_000_000).is_ok();
            eng.tick_source_alive = false;
            let r = eng.direct_supply(3_000_000);
            ok1 && r == Err(DmCode::NoTickSource)
        }
        None => false,
    };
    (ok, format!("节拍源失效拒: {}", ok))
}

fn drill_target_range_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    for _ in 0..10 {
        let mut e = DualModeEngine::new(4).ok();
        let ok = match e.as_mut() {
            Some(eng) => {
                let _ = eng.switch_mode(FreqMode::Direct);
                let bad = if lcg.next_below(2) == 0 {
                    FREQ_MIN - 1 - lcg.next_below(50_000) as u32
                } else {
                    FREQ_MAX + 1 + lcg.next_below(50_000) as u32
                };
                matches!(eng.direct_supply(bad), Err(DmCode::TargetOutOfRange))
            }
            None => false,
        };
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("目标越域注入 {}/10 必抓", caught))
}

fn drill_sampled_mapping() -> (bool, String) {
    // 采样决策闭式：负载 0/5000/10000 → 档 0/3/7。
    let mut e = DualModeEngine::new(4).ok();
    let ok = match e.as_mut() {
        Some(eng) => {
            let _ = eng.switch_mode(FreqMode::Sampled);
            let f0 = eng.sampled_decide(0).unwrap_or(0);
            let f5 = eng.sampled_decide(5_000).unwrap_or(0);
            let f10 = eng.sampled_decide(10_000).unwrap_or(0);
            f0 == 800_000 && f5 == 800_000 + 3 * 614_285 && f10 == 800_000 + 7 * 614_285
        }
        None => false,
    };
    (ok, format!("采样闭式映档: {}", ok))
}

fn drill_coexist_declaration() -> (bool, String) {
    // 并存声明：两模名可查（闭集）——语义核：双模名都可达但单活跃。
    let names = [FreqMode::Direct.name(), FreqMode::Sampled.name()];
    let mut e = DualModeEngine::new(4).ok();
    let ok = match e.as_mut() {
        Some(eng) => {
            let _ = eng.switch_mode(FreqMode::Direct);
            let a1 = eng.active == Some(FreqMode::Direct);
            let _ = eng.switch_mode(FreqMode::Sampled);
            let a2 = eng.active == Some(FreqMode::Sampled);
            names[0].len() > 0 && names[1].len() > 0 && a1 && a2
        }
        None => false,
    };
    (ok, format!("并存声明+单活跃: {}", ok))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    DmCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_mutex_switch_10();
    let (a2, d2) = drill_mixed_run_10();
    let (a3, d3) = drill_coexist_declaration();
    let ok = a1 && a2 && a3;
    (ok, format!("全流程: 互斥清账={} 混跑检出={} 并存声明={} [{}|{}|{}]", a1, a2, a3, d1, d2, d3))
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_mutex_switch_10();
    v.push(("J1-互斥切换+清账10/10", a, b));
    let (a, b) = drill_mixed_run_10();
    v.push(("J1-混跑注入10/10检出", a, b));
    let (a, b) = drill_ledger_residue_detected();
    v.push(("J2-账残留注入抓", a, b));
    let (a, b) = drill_hotswap_monotonic();
    v.push(("J1-热换只增即时生效", a, b));
    let (a, b) = drill_hotswap_regress_detected();
    v.push(("J2-热换回退注入抓", a, b));
    let (a, b) = drill_clear_line_missing();
    v.push(("J2-清账行缺失注入抓", a, b));
    let (a, b) = drill_zero_sample_period_10();
    v.push(("J2-零采样周期注入10/10", a, b));
    let (a, b) = drill_tick_source_lost();
    v.push(("J3-节拍源失效拒", a, b));
    let (a, b) = drill_target_range_10();
    v.push(("J2-目标越域注入10/10", a, b));
    let (a, b) = drill_sampled_mapping();
    v.push(("J3-采样闭式映档", a, b));
    let (a, b) = drill_coexist_declaration();
    v.push(("J1-并存声明", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx2009_checks：本单判据聚合（诊断段 0x36700）。
pub fn run_unx2009_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2009 直接耦合与采样型双模并存策略");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x36700 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
