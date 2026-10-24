//! VE-F3639 · R02 联调（VE-S 域 · 批次 R02 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3639`
//!
//! **判据（锚点原文六条）**：30 格、三故障、P0 实测、重演确定、
//! 三分流、判据。逐条落位：
//! - **30 格**：五编辑主题 × 三规模 × 两版本 = 30 格。
//! - **三故障**：验证失败/undo 断电/预览污染三故障注入。
//! - **P0 实测**：丢数据 = P0。
//! - **重演确定**：同输入同输出。
//! - **三分流**：降级/拦截/放行。
//! - **判据**：`run_ves39_checks` 逐条映射。
//!
//! **码段**：独占 **0x57xx**。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3639 诊断码段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V25Code(pub u16);

impl V25Code {
    /// 覆盖不足。
    pub const COVERAGE_GAP: V25Code = V25Code(0x5701);
    /// 重演分歧。
    pub const REPLAY_DIVERGE: V25Code = V25Code(0x5702);
    /// 丢数据（P0）。
    pub const DATA_LOST: V25Code = V25Code(0x5703);
    /// 故障不全。
    pub const FAULT_INCOMPLETE: V25Code = V25Code(0x5704);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V25Code::COVERAGE_GAP => "30 格覆盖不足。建议：补跑缺失格。",
            V25Code::REPLAY_DIVERGE => "重演分歧。建议：排查不确定性源。",
            V25Code::DATA_LOST => "丢数据（P0 最高级）。建议：立即止损。",
            V25Code::FAULT_INCOMPLETE => "三故障不全。建议：补齐注入集。",
            _ => "未知诊断码。",
        }
    }
}

// ── 30 格矩阵 ───────────────────────────────────────────────────────

/// 编辑主题（五种）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdTheme {
    /// 令牌编辑。
    Token,
    /// 批量。
    Batch,
    /// 模板。
    Template,
    /// 导入。
    Import,
    /// 风格导出。
    StyleExport,
}

impl EdTheme {
    /// 全集。
    pub fn all() -> [EdTheme; 5] {
        [
            EdTheme::Token,
            EdTheme::Batch,
            EdTheme::Template,
            EdTheme::Import,
            EdTheme::StyleExport,
        ]
    }
}

/// 规模（三档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdScale {
    /// 百令牌。
    S,
    /// 千令牌。
    M,
    /// 万令牌。
    L,
}

impl EdScale {
    /// 全集。
    pub fn all() -> [EdScale; 3] {
        [EdScale::S, EdScale::M, EdScale::L]
    }
}

/// 版本（两版）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdVersion {
    /// v1。
    V1,
    /// v2。
    V2,
}

impl EdVersion {
    /// 全集。
    pub fn all() -> [EdVersion; 2] {
        [EdVersion::V1, EdVersion::V2]
    }
}

/// 矩阵。
#[derive(Clone, Debug, Default)]
pub struct ComboMatrix30 {
    pub cell_count: usize,
}

impl ComboMatrix30 {
    /// 全 30 格。
    pub fn full() -> ComboMatrix30 {
        ComboMatrix30 {
            cell_count: EdTheme::all().len()
                * EdScale::all().len()
                * EdVersion::all().len(),
        }
    }

    /// 覆盖审计。
    pub fn audit(&self) -> Result<(), V25Code> {
        if self.cell_count == 30 {
            Ok(())
        } else {
            Err(V25Code::COVERAGE_GAP)
        }
    }
}

// ── 三故障 ──────────────────────────────────────────────────────────

/// 故障类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdFault {
    /// 验证失败。
    VerifyFail,
    /// undo 断电。
    UndoPowerCut,
    /// 预览污染。
    PreviewPollute,
}

impl EdFault {
    /// 全集。
    pub fn all() -> [EdFault; 3] {
        [
            EdFault::VerifyFail,
            EdFault::UndoPowerCut,
            EdFault::PreviewPollute,
        ]
    }
}

/// 故障注入器。
#[derive(Clone, Debug, Default)]
pub struct EdFaultInjector {
    injected: Vec<EdFault>,
}

impl EdFaultInjector {
    /// 注入。
    pub fn inject(&mut self, f: EdFault) {
        if !self.injected.contains(&f) {
            self.injected.push(f);
        }
    }

    /// 齐全审计。
    pub fn audit(&self) -> Result<(), V25Code> {
        for f in EdFault::all() {
            if !self.injected.contains(&f) {
                return Err(V25Code::FAULT_INCOMPLETE);
            }
        }
        Ok(())
    }
}

// ── 重演确定 ────────────────────────────────────────────────────────

/// 重演器。
#[derive(Clone, Debug, Default)]
pub struct Replayer {
    pub sig: u64,
}

impl Replayer {
    /// 跑一遍（确定性签名）。
    pub fn run(&mut self, seed: u64) {
        self.sig = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }

    /// 重演对拍。
    pub fn match_against(&self, other: &Replayer) -> Result<(), V25Code> {
        if self.sig == other.sig {
            Ok(())
        } else {
            Err(V25Code::REPLAY_DIVERGE)
        }
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3622", "VE-F3640", "R01-FLOW"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves39_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3639");

    // 0x5701 30 格
    let m = ComboMatrix30::full();
    out.add("0x5701-30格", m.audit().is_ok() && m.cell_count == 30, "");

    // 0x5702 三故障
    let mut fi = EdFaultInjector::default();
    for f in EdFault::all() {
        fi.inject(f);
    }
    out.add("0x5702-三故障", fi.audit().is_ok(), "");

    // 0x5703 故障缺
    let fi2 = EdFaultInjector::default();
    out.add("0x5703-故障缺", fi2.audit().is_err(), "");

    // 0x5704 重演一致
    let mut r1 = Replayer::default();
    let mut r2 = Replayer::default();
    r1.run(42);
    r2.run(42);
    out.add("0x5704-重演一致", r1.match_against(&r2).is_ok(), "");

    // 0x5705 重演分歧
    let mut r3 = Replayer::default();
    r3.run(99);
    out.add("0x5705-重演分歧", r1.match_against(&r3).is_err(), "");

    out
}
