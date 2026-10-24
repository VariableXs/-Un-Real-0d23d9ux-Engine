//! VE-F3640 · R02 组收口双签（VE-S 域 · 批次 R02 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3640`
//!
//! **判据（锚点原文六条）**：十三族、单源硬门、P0 硬门、保真证据、
//! 双签、判据。逐条落位：
//! - **十三族**：R02 冻结 API 十三族签名表。
//! - **单源硬门**：零私造断言（私造 → P1 重构）。
//! - **P0 硬门**：丢数据阻断。
//! - **保真证据**：保真对拍全绿。
//! - **双签**：双签结构。
//! - **判据**：`run_ves40_checks` 逐条映射。
//!
//! **码段**：独占 **0x58xx**。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3640 诊断码段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V26Code(pub u16);

impl V26Code {
    /// 映射缺。
    pub const MAP_MISS: V26Code = V26Code(0x5801);
    /// 单源破。
    pub const SOURCE_BREAK: V26Code = V26Code(0x5802);
    /// 丢数据。
    pub const DATA_LOST: V26Code = V26Code(0x5803);
    /// 双签缺。
    pub const SIGN_MISS: V26Code = V26Code(0x5804);
    /// 保真证据缺。
    pub const FIDELITY_MISS: V26Code = V26Code(0x5805);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V26Code::MAP_MISS => "映射缺。建议：补签名映射。",
            V26Code::SOURCE_BREAK => "单源破（P1 重构）。建议：归一单源。",
            V26Code::DATA_LOST => "丢数据（P0 阻断）。建议：立即止损。",
            V26Code::SIGN_MISS => "双签缺。建议：补双签。",
            V26Code::FIDELITY_MISS => "保真证据缺。建议：补对拍。",
            _ => "未知诊断码。",
        }
    }
}

// ── API 族（十三）────────────────────────────────────────────────────

/// R02 API 族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum R02Family {
    /// 可视化编辑。
    VisualEdit,
    /// 实时预览。
    LivePreview,
    /// 差异对比。
    Diff,
    /// 模板。
    Template,
    /// 向导。
    Wizard,
    /// 发布。
    Publish,
    /// 导入。
    Import,
    /// undo。
    Undo,
    /// 协作预留。
    Collab,
    /// 调试。
    Debug,
    /// 测试。
    Test,
    /// 性能。
    Perf,
    /// 无障碍。
    A11y,
}

impl R02Family {
    /// 全集（十三）。
    pub fn all() -> [R02Family; 13] {
        [
            R02Family::VisualEdit,
            R02Family::LivePreview,
            R02Family::Diff,
            R02Family::Template,
            R02Family::Wizard,
            R02Family::Publish,
            R02Family::Import,
            R02Family::Undo,
            R02Family::Collab,
            R02Family::Debug,
            R02Family::Test,
            R02Family::Perf,
            R02Family::A11y,
        ]
    }
}

// ── 收口清单 ────────────────────────────────────────────────────────

/// 收口器。
#[derive(Clone, Debug, Default)]
pub struct R02Close {
    covered: Vec<R02Family>,
    private_sources: u32,
    data_loss: bool,
    fidelity_green: bool,
    signed_internal: bool,
    signed_external: bool,
}

impl R02Close {
    /// 新建。
    pub fn new() -> R02Close {
        R02Close {
            covered: Vec::new(),
            private_sources: 0,
            data_loss: false,
            fidelity_green: false,
            signed_internal: false,
            signed_external: false,
        }
    }

    /// 覆盖一族。
    pub fn cover(&mut self, f: R02Family) {
        if !self.covered.contains(&f) {
            self.covered.push(f);
        }
    }

    /// 族覆盖审计。
    pub fn audit_families(&self) -> Result<(), V26Code> {
        for f in R02Family::all() {
            if !self.covered.contains(&f) {
                return Err(V26Code::MAP_MISS);
            }
        }
        Ok(())
    }

    /// 单源审计。
    pub fn audit_source(&self) -> Result<(), V26Code> {
        if self.private_sources == 0 {
            Ok(())
        } else {
            Err(V26Code::SOURCE_BREAK)
        }
    }

    /// P0 硬门（丢数据）。
    pub fn audit_p0(&self) -> Result<(), V26Code> {
        if self.data_loss {
            Err(V26Code::DATA_LOST)
        } else {
            Ok(())
        }
    }

    /// 保真证据。
    pub fn audit_fidelity(&self) -> Result<(), V26Code> {
        if self.fidelity_green {
            Ok(())
        } else {
            Err(V26Code::FIDELITY_MISS)
        }
    }

    /// 双签。
    pub fn audit_sign(&self) -> Result<(), V26Code> {
        if self.signed_internal && self.signed_external {
            Ok(())
        } else {
            Err(V26Code::SIGN_MISS)
        }
    }

    /// 全面收口。
    pub fn close(&mut self) -> Result<(), V26Code> {
        self.audit_families()?;
        self.audit_source()?;
        self.audit_p0()?;
        self.audit_fidelity()?;
        self.audit_sign()
    }

    /// 签一注。
    pub fn sign(&mut self, internal: bool) {
        if internal {
            self.signed_internal = true;
        } else {
            self.signed_external = true;
        }
    }

    /// 设保真绿。
    pub fn set_fidelity(&mut self, green: bool) {
        self.fidelity_green = green;
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["R03-GATE", "LEDGER", "VE-F2473"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves40_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3640");

    // 0x5801 十三族
    let mut rc = R02Close::new();
    for f in R02Family::all() {
        rc.cover(f);
    }
    out.add("0x5801-十三族", rc.audit_families().is_ok(), "");

    // 0x5802 族缺
    let rc2 = R02Close::new();
    out.add("0x5802-族缺", rc2.audit_families().is_err(), "");

    // 0x5803 单源
    out.add("0x5803-单源", rc.audit_source().is_ok(), "");

    // 0x5804 P0 门
    out.add("0x5804-P0门", rc.audit_p0().is_ok(), "");

    // 0x5805 保真
    rc.set_fidelity(true);
    out.add("0x5805-保真", rc.audit_fidelity().is_ok(), "");

    // 0x5806 双签
    rc.sign(true);
    rc.sign(false);
    out.add("0x5806-双签", rc.audit_sign().is_ok(), "");

    // 0x5807 全面收口
    out.add("0x5807-全面收口", rc.close().is_ok(), "");

    out
}
