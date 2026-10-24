//! UNX-F0233 · A1 域完工判据集（20 维对接）
//!
//! 锚点判据（施工书原文）：UNX-F0233-J1 域级 20 维判据（引导成功
//! 率/断言面/报告零差异/红线零违例）聚合面板生成。
//!
//! 设计：20 维判据表（每维 = 名称 + 权重 + 达标线 + 实测值）→
//! 聚合面板（逐维达标核对 + 加权总分）→ 红线：①20 维缺一即不
//! 出面板；②任一维未达红线即整面板红（不许加权总分掩盖单维
//! 失守）；③实测值越界（>100 或 <0）拒收。
//!
//! 错误码段：0x2D90（全仓普查后独占分配，AI-WBX1022）。

use alloc::vec::Vec;

/// 模块版本。
pub const F0233_VERSION: &str = "F0233-a1final-v1";

/// 诊断码段基址（UNX-F0233 独占）。
pub const SEG_BASE: u16 = 0x2D90;

/// 判据维数（判据口径 20）。
pub const DIMS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 维缺失（不足 20）。
    DimShort = SEG_BASE + 1,
    /// 单维失守（红线维度未达）。
    DimFailed = SEG_BASE + 2,
    /// 实测值越界。
    ValueOut = SEG_BASE + 3,
    /// 维重复。
    DimDup = SEG_BASE + 4,
    /// 面板未生成先出结论。
    PanelMissing = SEG_BASE + 5,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 单维判据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dim {
    /// 维名。
    pub name: &'static str,
    /// 达标线（千分比 0~1000）。
    pub bar: u16,
    /// 实测值（千分比 0~1000）。
    pub value: u16,
    /// 红线维（失守即整面板红）。
    pub redline: bool,
}

impl Dim {
    /// 达标。
    pub fn met(&self) -> bool {
        self.value >= self.bar
    }
}

/// A1 域 20 维标准清单（名称真源）。
pub const DIM_NAMES: [&str; 20] = [
    "boot-success",
    "assert-surface",
    "report-zero-diff",
    "redline-zero-violation",
    "readonly-boot",
    "sb-chain",
    "media-pick",
    "rollback",
    "sig-verify",
    "fp-stable",
    "log-trace",
    "json-export",
    "desens",
    "perf-budget",
    "probe-coverage",
    "bss-audit",
    "timeline-closed",
    "drill-badimg",
    "drill-powcut",
    "handoff-consumers",
];

/// 聚合面板。
#[derive(Debug, Default)]
pub struct A1Panel {
    /// 维表。
    pub dims: Vec<Dim>,
    /// 面板已生成。
    pub generated: bool,
}

impl A1Panel {
    /// 申报一维（重复拒、越界拒）。
    pub fn declare(&mut self, d: Dim) -> Result<(), u16> {
        if d.value > 1000 || d.bar > 1000 {
            return Err(E::ValueOut.code());
        }
        if self.dims.iter().any(|x| x.name == d.name) {
            return Err(E::DimDup.code());
        }
        self.dims.push(d);
        Ok(())
    }

    /// 生成面板：20 维齐 + 全部达标才绿（红线维失守单独显性）。
    pub fn generate(&mut self) -> Result<u64, u16> {
        if self.dims.len() != DIMS {
            return Err(E::DimShort.code());
        }
        // 名称真源对拍（20 名称一一在场）。
        for n in DIM_NAMES.iter() {
            if !self.dims.iter().any(|d| d.name == *n) {
                return Err(E::DimShort.code());
            }
        }
        // 红线维优先显性（不许加权掩盖）。
        for d in self.dims.iter() {
            if d.redline && !d.met() {
                return Err(E::DimFailed.code());
            }
        }
        // 全维达标核对。
        for d in self.dims.iter() {
            if !d.met() {
                return Err(E::DimFailed.code());
            }
        }
        // 加权总分（红线维权重 2，普通 1）。
        let total: u64 = self
            .dims
            .iter()
            .map(|d| d.value as u64 * if d.redline { 2 } else { 1 })
            .sum();
        self.generated = true;
        Ok(total)
    }

    /// 结论出口：面板未生成拒。
    pub fn verdict(&self) -> Result<(), u16> {
        if !self.generated {
            return Err(E::PanelMissing.code());
        }
        Ok(())
    }
}

/// J1 自证（判据原文：20 维判据聚合面板生成）。
pub fn self_audit() -> Result<(), u16> {
    // 一、夹具：20 维全绿（红线维 4 项全达）。
    let mut p = A1Panel::default();
    for (i, n) in DIM_NAMES.iter().enumerate() {
        p.declare(Dim {
            name: n,
            bar: 1000,
            value: 1000,
            redline: i < 4,
        })?;
    }

    // 二、J1 主体：面板生成 + 总分 = 20×1000 + 红线加成 4×1000 = 24000。
    let total = p.generate()?;
    if total != 24_000 {
        return Err(E::DimFailed.code());
    }
    p.verdict()?;

    // 三、红线维失守显性（即便其余 19 维全绿）。
    let mut rp = A1Panel::default();
    for (i, n) in DIM_NAMES.iter().enumerate() {
        rp.declare(Dim {
            name: n,
            bar: 1000,
            value: if i == 3 { 999 } else { 1000 }, // redline-zero-violation 失守。
            redline: i < 4,
        })?;
    }
    if rp.generate() != Err(E::DimFailed.code()) {
        return Err(E::DimFailed.code());
    }

    // 四、红线逐项：维缺失 / 维重复 / 越界 / 面板未生成。
    let mut short = A1Panel::default();
    for (i, n) in DIM_NAMES.iter().enumerate() {
        if i >= 19 {
            break;
        }
        short.declare(Dim {
            name: n,
            bar: 1000,
            value: 1000,
            redline: false,
        })?;
    }
    if short.generate().is_ok() {
        return Err(E::DimShort.code());
    }
    let mut dupp = A1Panel::default();
    dupp.declare(Dim {
        name: "boot-success",
        bar: 1000,
        value: 1000,
        redline: true,
    })?;
    if dupp
        .declare(Dim {
            name: "boot-success",
            bar: 1000,
            value: 1000,
            redline: true,
        })
        .is_ok()
    {
        return Err(E::DimDup.code());
    }
    if dupp
        .declare(Dim {
            name: "x",
            bar: 1001,
            value: 1000,
            redline: false,
        })
        .is_ok()
    {
        return Err(E::ValueOut.code());
    }
    if dupp.verdict().is_ok() {
        return Err(E::PanelMissing.code());
    }

    // 五、判别式段位对拍。
    if E::DimShort.code() != SEG_BASE + 1 || E::PanelMissing.code() != SEG_BASE + 5 {
        return Err(E::DimShort.code());
    }
    Ok(())
}
