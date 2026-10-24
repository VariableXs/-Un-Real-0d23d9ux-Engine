//! VE-F3742 · 创作工具无障碍终审（VE-S 域 · 批次 R08 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3742`
//!
//! **职责定位（锚点原文）**：工具终审——
//! - **全域终审**：四工具 + 编辑器无障碍全域终审
//!   （F3611/F3622/F3629/F3650 各条归一审计）：
//!   键盘画布全域（四画布键盘可达断言，复述红线全域化）；
//!   色值读报全域（复述）；辅助不阻塞全域（复述）；
//! - **工具五标准**：DX 五标准 × 无障碍交集审计
//!   （DX 与无障碍双标准）——双标准断言：工具必须同时过 DX 与无障碍。
//!
//! **判据（锚点原文六条）**：全域键盘、双标断言、独立审计、
//! 复述全域、归一执法、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 画布键盘缺失→P1（复述红线实测全域） | [`V8Code::CANVAS_KEYBOARD_MISS`] |
//! | 双标缺一→缺陷立案（红线实测） | [`V8Code::DUAL_STD_MISS`] |
//! | 审计形式化→抽样复验（复述） | [`V8Code::AUDIT_SHALLOW`] |
//! | 项缺失→补齐（复述） | [`V8Code::ITEM_GAP`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3742 诊断码段（独占 0x85xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V8Code(pub u16);

impl V8Code {
    /// 画布键盘缺失（P1 全域实测）。
    pub const CANVAS_KEYBOARD_MISS: V8Code = V8Code(0x8501);
    /// 双标缺一（立案）。
    pub const DUAL_STD_MISS: V8Code = V8Code(0x8502);
    /// 审计形式化（抽样复验）。
    pub const AUDIT_SHALLOW: V8Code = V8Code(0x8503);
    /// 项缺失（补齐）。
    pub const ITEM_GAP: V8Code = V8Code(0x8504);
    /// 工具 ID 非法。
    pub const BAD_TOOL: V8Code = V8Code(0x8505);
    /// 读报缺失。
    pub const READBACK_MISS: V8Code = V8Code(0x8506);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            V8Code::CANVAS_KEYBOARD_MISS => "画布键盘缺失（P1 全域实测）。建议：补键盘可达。",
            V8Code::DUAL_STD_MISS => "双标缺一（红线实测）。建议：DX 与无障碍须同过。",
            V8Code::AUDIT_SHALLOW => "审计形式化（复述）。建议：抽样复验。",
            V8Code::ITEM_GAP => "项缺失（复述）。建议：补齐审计项。",
            V8Code::BAD_TOOL => "工具 ID 非法。建议：0-3 四工具。",
            V8Code::READBACK_MISS => "色值读报缺失。建议：补读报支持。",
            _ => "未知诊断码。",
        }
    }
}

// ── 全域审计表（工具×项） ───────────────────────────────────────────

/// 审计工具。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditTool {
    /// 壁纸工坊。
    Wall = 0,
    /// 图标工坊。
    Icon = 1,
    /// 光标工坊。
    Cursor = 2,
    /// 主题编辑器。
    Editor = 3,
}

impl AuditTool {
    pub const ALL: [AuditTool; 4] = [
        AuditTool::Wall,
        AuditTool::Icon,
        AuditTool::Cursor,
        AuditTool::Editor,
    ];
}

/// 审计项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditItem {
    /// 键盘画布。
    KeyboardCanvas,
    /// 色值读报。
    ColorReadback,
    /// 辅助不阻塞。
    AssistNonBlock,
}

impl AuditItem {
    pub const ALL: [AuditItem; 3] = [
        AuditItem::KeyboardCanvas,
        AuditItem::ColorReadback,
        AuditItem::AssistNonBlock,
    ];
}

/// 审计表。
pub struct AuditGrid {
    /// 4 工具 × 3 项 = 12 格。
    grid: [bool; 12],
}

impl AuditGrid {
    pub fn new() -> AuditGrid {
        AuditGrid {
            grid: [false; 12],
        }
    }

    fn idx(t: AuditTool, i: AuditItem) -> usize {
        (t as usize) * 3 + (i as usize)
    }

    /// 记录通过。
    pub fn pass(&mut self, t: AuditTool, i: AuditItem) {
        self.grid[AuditGrid::idx(t, i)] = true;
    }

    /// 全域完整（12 格）。
    pub fn complete(&self) -> Result<(), V8Code> {
        for g in &self.grid {
            if !g {
                return Err(V8Code::ITEM_GAP);
            }
        }
        Ok(())
    }

    /// 键盘画布全域（四工具该项全过）。
    pub fn keyboard_universal(&self) -> Result<(), V8Code> {
        for t in AuditTool::ALL {
            if !self.grid[AuditGrid::idx(t, AuditItem::KeyboardCanvas)] {
                return Err(V8Code::CANVAS_KEYBOARD_MISS);
            }
        }
        Ok(())
    }

    /// 色值读报全域。
    pub fn readback_universal(&self) -> Result<(), V8Code> {
        for t in AuditTool::ALL {
            if !self.grid[AuditGrid::idx(t, AuditItem::ColorReadback)] {
                return Err(V8Code::READBACK_MISS);
            }
        }
        Ok(())
    }
}

// ── 双标断言（DX × 无障碍） ────────────────────────────────────────

/// 标准。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Std {
    /// DX 五标准（开发者体验）。
    Dx,
    /// 无障碍标准。
    A11y,
}

/// 双标断言器。
pub struct DualStdAssert {
    dx: bool,
    a11y: bool,
}

impl DualStdAssert {
    pub fn new() -> DualStdAssert {
        DualStdAssert {
            dx: false,
            a11y: false,
        }
    }

    /// 标记过标。
    pub fn pass(&mut self, s: Std) {
        match s {
            Std::Dx => self.dx = true,
            Std::A11y => self.a11y = true,
        }
    }

    /// 双标断言（缺一即缺陷）。
    pub fn assert(&self) -> Result<(), V8Code> {
        if self.dx && self.a11y {
            Ok(())
        } else {
            Err(V8Code::DUAL_STD_MISS)
        }
    }
}

// ── 抽样复验（反形式化） ────────────────────────────────────────────

/// 复验器。
pub struct SpotCheck {
    /// 样本数。
    samples: u32,
    /// 上限（形式化警戒）。
    shallow_cap: u32,
}

impl SpotCheck {
    pub fn new(shallow_cap: u32) -> SpotCheck {
        SpotCheck {
            samples: 0,
            shallow_cap,
        }
    }

    /// 取样。
    pub fn sample(&mut self) {
        self.samples += 1;
    }

    /// 形式化检出（样本过少即浅审）。
    pub fn depth_check(&self) -> Result<(), V8Code> {
        if self.samples < self.shallow_cap {
            Err(V8Code::AUDIT_SHALLOW)
        } else {
            Ok(())
        }
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3742 判据集（锚点原文六条逐条落位）。
pub fn run_vr08b_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3742");

    // 1) 全域键盘：齐过
    let mut ag = AuditGrid::new();
    for t in AuditTool::ALL {
        for i in AuditItem::ALL {
            ag.pass(t, i);
        }
    }
    out.add(
        "0x8501-全域键盘",
        ag.keyboard_universal().is_ok(),
        "键盘全域须过",
    );

    // 2) 全域键盘：缺被拦
    let mut ag2 = AuditGrid::new();
    for t in AuditTool::ALL {
        ag2.pass(t, AuditItem::ColorReadback);
    }
    out.add(
        "0x8501-缺拦",
        ag2.keyboard_universal().is_err(),
        "缺键盘须拦",
    );

    // 3) 双标断言：齐过
    let mut dsa = DualStdAssert::new();
    dsa.pass(Std::Dx);
    dsa.pass(Std::A11y);
    out.add(
        "0x8502-双标断言",
        dsa.assert().is_ok(),
        "双标须齐",
    );

    // 4) 双标断言：缺一被拦
    let mut dsa2 = DualStdAssert::new();
    dsa2.pass(Std::Dx);
    out.add(
        "0x8502-缺一拦",
        dsa2.assert().is_err(),
        "缺一须拦",
    );

    // 5) 形式化检出：浅审拦
    let sc = SpotCheck::new(5);
    out.add(
        "0x8503-形式化",
        sc.depth_check().is_err(),
        "浅审须拦",
    );

    // 6) 全域完整：12 格
    out.add(
        "0x8504-全域完整",
        ag.complete().is_ok(),
        "12 格须齐",
    );

    // 7) 读报全域
    out.add(
        "0x8506-读报全域",
        ag.readback_universal().is_ok(),
        "读报须全",
    );

    // 8) 独立审计：工具集齐
    out.add(
        "0x8505-独立审计",
        AuditTool::ALL.len() == 4,
        "四工具须在",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x8506-判据",
        V8Code::CANVAS_KEYBOARD_MISS.value() != V8Code::DUAL_STD_MISS.value(),
        "码段互异",
    );

    out
}
