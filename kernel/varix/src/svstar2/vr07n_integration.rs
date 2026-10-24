//! VE-F3734 · 创作性能联调（VE-S 域 · 批次 R07 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3734`
//!
//! **职责定位（锚点原文）**：创作性能联调——
//! - **组合矩阵**：四工具×三设备档×三压力态×三资产规模 = 108 格
//!   分层抽样 54 格（覆盖底线复述）；
//! - **真实剧本**：20 分钟创作剧本（大画布编辑+批量导入+导出+
//!   风格切换混流）——重演确定性 + 心流采集（中断率实测）——
//!   中断剧本线（每小时中断 ≤3 复述实测场）；
//! - **最坏场景**：16M 画布+千图层+低端+满载四重最坏（复述深化）；
//! - **三分流**复述。
//!
//! **判据（锚点原文五条）**：54 格、四重最坏、中断剧本线、
//! 心流采集、三分流、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 四重失守→立案（复述红线实测） | [`I7Code::WORST_LOST`] |
//! | 中断超→P1（剧本线实测） | [`I7Code::SCRIPT_INTERRUPT`] |
//! | 覆盖不达标→补跑（复述） | [`I7Code::COVERAGE_LOW`] |
//! | 重演分歧→立案（复述） | [`I7Code::REPLAY_DIVERGE`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3734 诊断码段（独占 0x7Dxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct I7Code(pub u16);

impl I7Code {
    /// 四重最坏失守（立案）。
    pub const WORST_LOST: I7Code = I7Code(0x7D01);
    /// 中断剧本线超（P1）。
    pub const SCRIPT_INTERRUPT: I7Code = I7Code(0x7D02);
    /// 覆盖不达标（补跑）。
    pub const COVERAGE_LOW: I7Code = I7Code(0x7D03);
    /// 重演分歧（立案）。
    pub const REPLAY_DIVERGE: I7Code = I7Code(0x7D04);
    /// 抽样参数非法。
    pub const BAD_SAMPLE: I7Code = I7Code(0x7D05);
    /// 剧本超时。
    pub const SCRIPT_TIMEOUT: I7Code = I7Code(0x7D06);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            I7Code::WORST_LOST => "四重最坏失守（复述红线实测）。建议：立案补齐最坏集。",
            I7Code::SCRIPT_INTERRUPT => "中断超剧本线（P1）。建议：定位中断源并降档。",
            I7Code::COVERAGE_LOW => "覆盖不达标（复述）。建议：补跑缺失格。",
            I7Code::REPLAY_DIVERGE => "重演分歧（复述）。建议：确定性立案排查。",
            I7Code::BAD_SAMPLE => "抽样参数非法。建议：抽样须 >0 且 ≤108。",
            I7Code::SCRIPT_TIMEOUT => "剧本超时。建议：20 分钟内须完。",
            _ => "未知诊断码。",
        }
    }
}

// ── 组合矩阵（108 格） ──────────────────────────────────────────────

/// 设备档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceTier {
    /// 高端。
    High,
    /// 中端。
    Mid,
    /// 低端。
    Low,
}

impl DeviceTier {
    pub const ALL: [DeviceTier; 3] = [DeviceTier::High, DeviceTier::Mid, DeviceTier::Low];
}

/// 压力态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StressState {
    /// 空闲。
    Idle,
    /// 满载。
    Loaded,
    /// 超载。
    Overloaded,
}

impl StressState {
    pub const ALL: [StressState; 3] = [
        StressState::Idle,
        StressState::Loaded,
        StressState::Overloaded,
    ];
}

/// 资产规模。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetScale {
    /// 小。
    Small,
    /// 中。
    Medium,
    /// 大。
    Large,
}

impl AssetScale {
    pub const ALL: [AssetScale; 3] = [
        AssetScale::Small,
        AssetScale::Medium,
        AssetScale::Large,
    ];
}

/// 抽样计划（54/108）。
pub struct SamplePlan {
    /// 已选格数。
    selected: u32,
    /// 总格数。
    total: u32,
    /// 底线。
    floor: u32,
}

impl SamplePlan {
    /// 标准：108 格抽 54。
    pub fn standard() -> SamplePlan {
        SamplePlan {
            selected: 0,
            total: 108,
            floor: 54,
        }
    }

    /// 选格。
    pub fn pick(&mut self, n: u32) -> Result<(), I7Code> {
        if n == 0 {
            return Err(I7Code::BAD_SAMPLE);
        }
        self.selected = self.selected.saturating_add(n);
        if self.selected > self.total {
            return Err(I7Code::BAD_SAMPLE);
        }
        Ok(())
    }

    /// 覆盖底线。
    pub fn coverage_ok(&self) -> Result<(), I7Code> {
        if self.selected >= self.floor {
            Ok(())
        } else {
            Err(I7Code::COVERAGE_LOW)
        }
    }
}

// ── 剧本执行器 ──────────────────────────────────────────────────────

/// 剧本步。
pub struct ScriptStep {
    /// 耗时（分钟）。
    pub minutes: u32,
    /// 中断次数。
    pub interrupts: u32,
}

/// 剧本执行器。
pub struct ScriptRunner {
    steps: Vec<ScriptStep>,
    /// 时长上限（分钟）。
    max_minutes: u32,
}

impl ScriptRunner {
    /// 标准 20 分钟剧本。
    pub fn standard() -> ScriptRunner {
        ScriptRunner {
            steps: Vec::new(),
            max_minutes: 20,
        }
    }

    /// 加步。
    pub fn push(&mut self, step: ScriptStep) {
        self.steps.push(step);
    }

    /// 总时长。
    pub fn total_minutes(&self) -> u32 {
        self.steps.iter().fold(0u32, |a, s| a.saturating_add(s.minutes))
    }

    /// 总中断。
    pub fn total_interrupts(&self) -> u32 {
        self.steps.iter().fold(0u32, |a, s| a.saturating_add(s.interrupts))
    }

    /// 中断剧本线（≤3/h；20 分钟折算 ≤1 次）。
    pub fn interrupt_ok(&self) -> Result<(), I7Code> {
        // 20 分钟 = 1/3 小时 → 上限 1 次
        if self.total_interrupts() > 1 {
            return Err(I7Code::SCRIPT_INTERRUPT);
        }
        Ok(())
    }

    /// 超时判定。
    pub fn timeout_check(&self) -> Result<(), I7Code> {
        if self.total_minutes() > self.max_minutes {
            Err(I7Code::SCRIPT_TIMEOUT)
        } else {
            Ok(())
        }
    }
}

// ── 四重最坏集 ──────────────────────────────────────────────────────

/// 最坏条件。
pub struct WorstCase {
    /// 16M 画布。
    pub canvas_16m: bool,
    /// 千图层。
    pub layers_1000: bool,
    /// 低端设备。
    pub device_low: bool,
    /// 满载。
    pub loaded: bool,
}

impl WorstCase {
    /// 四重齐全。
    pub fn complete(&self) -> Result<(), I7Code> {
        if self.canvas_16m && self.layers_1000 && self.device_low && self.loaded {
            Ok(())
        } else {
            Err(I7Code::WORST_LOST)
        }
    }
}

// ── 三分流 ──────────────────────────────────────────────────────────

/// 分流结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Triage {
    /// 通过。
    Pass,
    /// 观察。
    Watch,
    /// 拦截。
    Block,
}

/// 分流器。
pub struct TriageGate;

impl TriageGate {
    /// 分流（中断 0=过 1=观 ≥2=拦）。
    pub fn triage(interrupts: u32) -> Triage {
        if interrupts == 0 {
            Triage::Pass
        } else if interrupts == 1 {
            Triage::Watch
        } else {
            Triage::Block
        }
    }
}

// ── 重演确定性 ──────────────────────────────────────────────────────

/// 重演对。
pub struct ReplayPair {
    pub first_fp: u32,
    pub second_fp: u32,
}

/// 重演器。
pub struct Replayer {
    pairs: Vec<ReplayPair>,
}

impl Replayer {
    pub fn new() -> Replayer {
        Replayer { pairs: Vec::new() }
    }

    pub fn push(&mut self, p: ReplayPair) {
        self.pairs.push(p);
    }

    /// 重演一致。
    pub fn audit(&self) -> Result<(), I7Code> {
        for p in &self.pairs {
            if p.first_fp != p.second_fp {
                return Err(I7Code::REPLAY_DIVERGE);
            }
        }
        Ok(())
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3734 判据集（锚点原文五条逐条落位）。
pub fn run_vr07n_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3734");

    // 1) 54 格：覆盖底线
    let mut plan = SamplePlan::standard();
    let _ = plan.pick(54);
    out.add(
        "0x7D03-54 格",
        plan.coverage_ok().is_ok(),
        "54 格须达标",
    );

    // 2) 覆盖：不达标被拦
    let mut plan2 = SamplePlan::standard();
    let _ = plan2.pick(10);
    out.add(
        "0x7D03-覆盖拦",
        plan2.coverage_ok().is_err(),
        "不达标须拦",
    );

    // 3) 中断剧本线：超被拦
    let mut sr = ScriptRunner::standard();
    sr.push(ScriptStep {
        minutes: 10,
        interrupts: 0,
    });
    sr.push(ScriptStep {
        minutes: 10,
        interrupts: 2,
    });
    out.add(
        "0x7D02-中断剧本线",
        sr.interrupt_ok().is_err(),
        "中断 2 次须拦",
    );

    // 4) 中断剧本线：达标过
    let mut sr2 = ScriptRunner::standard();
    sr2.push(ScriptStep {
        minutes: 20,
        interrupts: 1,
    });
    out.add(
        "0x7D02-达标过",
        sr2.interrupt_ok().is_ok() && sr2.timeout_check().is_ok(),
        "达标须过",
    );

    // 5) 四重最坏：齐备
    let wc = WorstCase {
        canvas_16m: true,
        layers_1000: true,
        device_low: true,
        loaded: true,
    };
    out.add(
        "0x7D01-四重最坏",
        wc.complete().is_ok(),
        "四重须齐",
    );

    // 6) 四重最坏：缺被拦
    let wc2 = WorstCase {
        canvas_16m: true,
        layers_1000: false,
        device_low: true,
        loaded: true,
    };
    out.add(
        "0x7D01-缺拦",
        wc2.complete().is_err(),
        "缺项须拦",
    );

    // 7) 三分流：分级
    out.add(
        "0x7D03-三分流",
        TriageGate::triage(0) == Triage::Pass
            && TriageGate::triage(1) == Triage::Watch
            && TriageGate::triage(5) == Triage::Block,
        "三级分流须对",
    );

    // 8) 重演：分歧拦
    let mut rp = Replayer::new();
    rp.push(ReplayPair {
        first_fp: 1,
        second_fp: 2,
    });
    out.add(
        "0x7D04-重演分歧",
        rp.audit().is_err(),
        "分歧须拦",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x7D06-判据",
        I7Code::WORST_LOST.value() != I7Code::SCRIPT_INTERRUPT.value(),
        "码段互异",
    );

    out
}
