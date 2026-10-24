//! VE-F3630 · 编辑器无障碍（VE-S 域 · 批次 R02 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3630`
//!
//! **判据（锚点原文六条）**：色值读报、键盘精调、辅助不阻塞、自举复述、
//! 数值通道、判据。逐条落位：
//! - **色值读报**：色弱创作者可精确编辑颜色（数值通道）——
//!   [`ColorChannel`] 键盘输入 + 读屏报值，色值无输入框 →
//!   [`V1ECode::NO_NUMERIC`]（红线实测：只能鼠标拾色 = 排除色弱）。
//! - **键盘精调**：滑杆方向键步进 + Shift 精调（[`SliderKeys`]）。
//! - **辅助不阻塞**：对比度实时提示非阻塞（复述）。
//! - **自举复述**：预览界面读屏（自举）。
//! - **数值通道**：数值通道协议（[`ColorChannel`]）。
//! - **判据**：`run_ves30_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 色值无输入框→补（红线实测） | [`V1ECode::NO_NUMERIC`] |
//! | 读屏不报值→P1（红线实测） | [`V1ECode::NO_ANNOUNCE`] |
//! | 提示阻塞→修正（复述） | [`V1ECode::HINT_BLOCKS`] |
//! | 自举不过→修（复述） | [`V1ECode::SELFBOOT_FAIL`] |
//!
//! **码段**：独占 **0x4exx**。
//!
//! **性能逐项分解**：输入 O(1)；报值 O(1)；提示 O(1)；自举 O(审查)。
//!
//! **跨批对接点**：F3611 单源复用；F3046 键盘复用；F2895 对比度对端
//! ——钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3630 诊断码段（独占 0x4exx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V1ECode(pub u16);

impl V1ECode {
    /// 无数值输入通道。
    pub const NO_NUMERIC: V1ECode = V1ECode(0x4e01);
    /// 读屏不报值。
    pub const NO_ANNOUNCE: V1ECode = V1ECode(0x4e02);
    /// 提示阻塞创作。
    pub const HINT_BLOCKS: V1ECode = V1ECode(0x4e03);
    /// 自举不过。
    pub const SELFBOOT_FAIL: V1ECode = V1ECode(0x4e04);
    /// 精调步长非法。
    pub const BAD_STEP: V1ECode = V1ECode(0x4e05);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V1ECode::NO_NUMERIC => {
                "色值无数值输入通道（红线：只能鼠标拾色 = 排除色弱）。建议：补数值输入框。"
            }
            V1ECode::NO_ANNOUNCE => {
                "读屏不报色值（红线 P1）。建议：补 ARIA 报值。"
            }
            V1ECode::HINT_BLOCKS => {
                "提示阻塞创作。建议：改非阻塞通道。"
            }
            V1ECode::SELFBOOT_FAIL => {
                "自举（预览读屏）不过。建议：补预览语义标注。"
            }
            V1ECode::BAD_STEP => {
                "精调步长非法。建议：步长 1..=16。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 色值数值通道 ────────────────────────────────────────────────────

/// 色值通道。
#[derive(Clone, Debug)]
pub struct ColorChannel {
    /// 是否有数值输入框。
    pub has_numeric_input: bool,
    /// 是否读屏报值。
    pub announces: bool,
    /// 当前值 0..255。
    pub value: u8,
}

impl ColorChannel {
    /// 新建（默认全开）。
    pub fn new() -> ColorChannel {
        ColorChannel {
            has_numeric_input: true,
            announces: true,
            value: 0,
        }
    }

    /// 无障碍审计。
    pub fn audit(&self) -> Result<(), V1ECode> {
        if !self.has_numeric_input {
            return Err(V1ECode::NO_NUMERIC);
        }
        if !self.announces {
            return Err(V1ECode::NO_ANNOUNCE);
        }
        Ok(())
    }

    /// 数值设置（钳制到 0..255）。
    pub fn set(&mut self, v: u32) {
        self.value = if v > 255 { 255 } else { v as u8 };
    }
}

// ── 滑杆键盘精调 ────────────────────────────────────────────────────

/// 滑杆键盘控制。
#[derive(Clone, Debug)]
pub struct SliderKeys {
    /// 粗调步长。
    pub coarse: u8,
    /// 精调步长（Shift）。
    pub fine: u8,
    /// 当前值。
    pub value: u32,
}

impl SliderKeys {
    /// 新建。
    pub fn new() -> SliderKeys {
        SliderKeys {
            coarse: 4,
            fine: 1,
            value: 0,
        }
    }

    /// 精调审计（步长合法）。
    pub fn audit(&self) -> Result<(), V1ECode> {
        if self.coarse == 0 || self.coarse > 16 || self.fine == 0 || self.fine > 16 {
            Err(V1ECode::BAD_STEP)
        } else {
            Ok(())
        }
    }

    /// 方向键。
    pub fn arrow(&mut self, up: bool, shift: bool) {
        let step = if shift { self.fine } else { self.coarse } as u32;
        if up {
            self.value = self.value.saturating_add(step);
        } else {
            self.value = self.value.saturating_sub(step);
        }
    }
}

// ── 非阻塞提示 ──────────────────────────────────────────────────────

/// 对比度实时提示（非阻塞）。
#[derive(Clone, Debug, Default)]
pub struct ContrastHint {
    blocking: bool,
}

impl ContrastHint {
    /// 新建。
    pub fn new() -> ContrastHint {
        ContrastHint { blocking: false }
    }

    /// 发提示（非阻塞）。
    pub fn issue(&mut self, _msg: &str) -> bool {
        !self.blocking
    }

    /// 审计。
    pub fn audit(&self) -> Result<(), V1ECode> {
        if self.blocking {
            Err(V1ECode::HINT_BLOCKS)
        } else {
            Ok(())
        }
    }

    /// 反向语料：模拟阻塞。
    pub fn block_for_test(&mut self) {
        self.blocking = true;
    }
}

// ── 自举（预览读屏）─────────────────────────────────────────────────

/// 自举审计。
#[derive(Clone, Debug, Default)]
pub struct SelfBootAudit {
    pub preview_readable: bool,
}

impl SelfBootAudit {
    /// 审计。
    pub fn audit(&self) -> Result<(), V1ECode> {
        if self.preview_readable {
            Ok(())
        } else {
            Err(V1ECode::SELFBOOT_FAIL)
        }
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3611", "VE-F3046", "VE-F2895"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves30_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3630");

    // 0x4e01 数值通道全开
    let cc = ColorChannel::new();
    out.add("0x4e01-数值通道全开", cc.audit().is_ok(), "");

    // 0x4e02 无数值输入
    let cc2 = ColorChannel {
        has_numeric_input: false,
        ..ColorChannel::new()
    };
    out.add("0x4e02-无数值输入", cc2.audit().is_err(), "");

    // 0x4e03 不报值
    let cc3 = ColorChannel {
        announces: false,
        ..ColorChannel::new()
    };
    out.add("0x4e03-不报值", cc3.audit().is_err(), "");

    // 0x4e04 钳制
    let mut cc4 = ColorChannel::new();
    cc4.set(300);
    out.add("0x4e04-钳制", cc4.value == 255, "");

    // 0x4e05 步长合法
    let sk = SliderKeys::new();
    out.add("0x4e05-步长合法", sk.audit().is_ok(), "");

    // 0x4e06 精调
    let mut sk2 = SliderKeys::new();
    sk2.arrow(true, true);
    out.add("0x4e06-精调", sk2.value == 1, "");

    // 0x4e07 粗调
    let mut sk3 = SliderKeys::new();
    sk3.arrow(true, false);
    out.add("0x4e07-粗调", sk3.value == 4, "");

    // 0x4e08 提示非阻塞
    let ch = ContrastHint::new();
    out.add("0x4e08-提示非阻塞", ch.audit().is_ok(), "");

    // 0x4e09 阻塞检出
    let mut ch2 = ContrastHint::new();
    ch2.block_for_test();
    out.add("0x4e09-阻塞检出", ch2.audit().is_err(), "");

    // 0x4e0a 自举
    let sb = SelfBootAudit {
        preview_readable: true,
    };
    out.add("0x4e0a-自举", sb.audit().is_ok(), "");

    out
}
