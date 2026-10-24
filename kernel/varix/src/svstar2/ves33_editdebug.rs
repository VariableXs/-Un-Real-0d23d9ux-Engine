//! VE-F3633 · 编辑器调试（VE-S 域 · 批次 R02 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3633`
//!
//! **判据（锚点原文六条）**：三维仪表、反查复用、定位单源、家族十七、
//! 零常态、判据。逐条落位：
//! - **三维仪表**：令牌编辑历史/预览状态/验证状态三维检视。
//! - **反查复用**：编辑令牌→下游影响实时显示（复用 F3413 反查）。
//! - **定位单源**：验证失败→编辑器内定位跳转（复用 F3613 定位）。
//! - **家族十七**：调试器协议家族十七成员（复述 F2815）。
//! - **零常态**：仪表默认不采样（打开才采）。
//! - **判据**：`run_ves33_checks` 逐条映射。
//!
//! **码段**：独占 **0x51xx**。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3633 诊断码段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V21Code(pub u16);

impl V21Code {
    /// 定位缺失。
    pub const LOCATE_MISS: V21Code = V21Code(0x5101);
    /// 仪表分歧。
    pub const GAUGE_DRIFT: V21Code = V21Code(0x5102);
    /// 影响失真。
    pub const IMPACT_DRIFT: V21Code = V21Code(0x5103);
    /// 采样未开。
    pub const SAMPLING_OFF: V21Code = V21Code(0x5104);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V21Code::LOCATE_MISS => "定位缺失。建议：补全错误定位。",
            V21Code::GAUGE_DRIFT => "仪表与实际分歧。建议：以实际为准刷新仪表。",
            V21Code::IMPACT_DRIFT => "影响面数据失真。建议：核对 F3413 反查源。",
            V21Code::SAMPLING_OFF => "采样未开（零常态）。建议：按需开启。",
            _ => "未知诊断码。",
        }
    }
}

// ── 三维仪表 ────────────────────────────────────────────────────────

/// 仪表维度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GaugeDim {
    /// 编辑历史。
    History,
    /// 预览状态。
    Preview,
    /// 验证状态。
    Verify,
}

impl GaugeDim {
    /// 全集。
    pub fn all() -> [GaugeDim; 3] {
        [GaugeDim::History, GaugeDim::Preview, GaugeDim::Verify]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            GaugeDim::History => "编辑历史",
            GaugeDim::Preview => "预览状态",
            GaugeDim::Verify => "验证状态",
        }
    }
}

/// 三维仪表。
#[derive(Clone, Debug, Default)]
pub struct DebugGauge {
    values: Vec<(GaugeDim, u32)>,
    /// 是否采样（零常态：默认关）。
    pub sampling: bool,
}

impl DebugGauge {
    /// 新建（不采样）。
    pub fn new() -> DebugGauge {
        DebugGauge {
            values: Vec::new(),
            sampling: false,
        }
    }

    /// 开启采样。
    pub fn enable(&mut self) {
        self.sampling = true;
    }

    /// 记一维值。
    pub fn record(&mut self, dim: GaugeDim, value: u32) {
        if self.sampling {
            if let Some(slot) = self.values.iter_mut().find(|(d, _)| *d == dim) {
                slot.1 = value;
            } else {
                self.values.push((dim, value));
            }
        }
    }

    /// 三维齐审计。
    pub fn audit_dims(&self) -> Result<(), V21Code> {
        for d in GaugeDim::all() {
            if !self.values.iter().any(|(dd, _)| *dd == d) {
                return Err(V21Code::GAUGE_DRIFT);
            }
        }
        Ok(())
    }

    /// 读一维。
    pub fn read(&self, dim: GaugeDim) -> Option<u32> {
        self.values.iter().find(|(d, _)| *d == dim).map(|(_, v)| *v)
    }
}

// ── 影响反查（F3413 复用）───────────────────────────────────────────

/// 反查器。
#[derive(Clone, Debug, Default)]
pub struct ImpactLookup {
    table: Vec<(String, u32)>,
}

impl ImpactLookup {
    /// 新建。
    pub fn new() -> ImpactLookup {
        ImpactLookup {
            table: Vec::new(),
        }
    }

    /// 登记。
    pub fn register(&mut self, token: &str, impact: u32) {
        self.table.push((String::from(token), impact));
    }

    /// 反查。
    pub fn lookup(&self, token: &str) -> Result<u32, V21Code> {
        self.table
            .iter()
            .find(|(t, _)| t == token)
            .map(|(_, i)| *i)
            .ok_or(V21Code::IMPACT_DRIFT)
    }
}

// ── 定位跳转（F3613 单源）───────────────────────────────────────────

/// 定位跳转。
#[derive(Clone, Debug, Default)]
pub struct LocateJump {
    targets: Vec<(u32, String)>,
}

impl LocateJump {
    /// 新建。
    pub fn new() -> LocateJump {
        LocateJump {
            targets: Vec::new(),
        }
    }

    /// 注册跳转目标。
    pub fn register(&mut self, issue: u32, target: &str) {
        self.targets.push((issue, String::from(target)));
    }

    /// 跳转。
    pub fn jump(&self, issue: u32) -> Result<&str, V21Code> {
        self.targets
            .iter()
            .find(|(i, _)| *i == issue)
            .map(|(_, t)| t.as_str())
            .ok_or(V21Code::LOCATE_MISS)
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 家族协议索引（十七）。
pub const FAMILY_MEMBER_INDEX: u16 = 17;

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3613", "VE-F3413", "VE-F2815"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves33_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3633");

    // 0x5101 三维仪表
    let mut g = DebugGauge::new();
    g.enable();
    for d in GaugeDim::all() {
        g.record(d, 1);
    }
    out.add("0x5101-三维仪表", g.audit_dims().is_ok(), "");

    // 0x5102 零常态
    let g2 = DebugGauge::new();
    out.add("0x5102-零常态", !g2.sampling, "");

    // 0x5103 未采样不记录
    let mut g3 = DebugGauge::new();
    g3.record(GaugeDim::History, 9);
    out.add("0x5103-未采样不记录", g3.read(GaugeDim::History).is_none(), "");

    // 0x5104 反查
    let mut il = ImpactLookup::new();
    il.register("primary", 5);
    out.add("0x5104-反查", il.lookup("primary") == Ok(5), "");

    // 0x5105 反查缺失
    out.add("0x5105-反查缺失", il.lookup("ghost").is_err(), "");

    // 0x5106 定位跳转
    let mut lj = LocateJump::new();
    lj.register(1, "layers.3.fill");
    out.add("0x5106-定位跳转", lj.jump(1).is_ok(), "");

    // 0x5107 跳转缺失
    out.add("0x5107-跳转缺失", lj.jump(99).is_err(), "");

    out
}
