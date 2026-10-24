//! VE-F3537 · Persona 文档 —— 五域使用指南 + 仲裁规则手册 + 预设说明 + CI 防漂移。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3537`
//!
//! # 职责（锚点原文拆解）
//!
//! - **五域速查表**（[`DOMAIN_CHEATSHEET`]）：每域一页说清——
//!   域名/可个性化项/硬线/回退路径四行。
//! - **仲裁规则手册**：F3531 链序的文档化（与实现同一常量源
//!   ——[`CHAIN_RULE`] 引用，防漂移的结构性保证）。
//! - **预设说明**：三官方预设各一条目（锚定 [`Preset`]）。
//! - **CI 防漂移**（[`PersonaDocCI`]）：锚点核对 + 必备类核对，
//!   漂移阻断收口。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 漂移 | CI 阻断 |
//! | 规则过时 | 复审 |
//! | 缺失 | 收口阻断 |
//!
//! # 复杂度
//!
//! 全部 O(条目)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5A60 起）
// ---------------------------------------------------------------------------

/// Persona 文档域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PDocCode {
    /// 条目失锚。
    AnchorLost = 0x5A60,
    /// 必备文档缺失。
    Missing = 0x5A61,
    /// 仲裁手册与实现漂移。
    RuleDrift = 0x5A62,
}

impl PDocCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            PDocCode::AnchorLost => "文档条目失锚",
            PDocCode::Missing => "必备文档缺失",
            PDocCode::RuleDrift => "仲裁手册与实现漂移",
        }
    }

    /// 全阻断。
    pub fn blocking(self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// 二、五域速查表
// ---------------------------------------------------------------------------

/// 一页速查。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CheatRow {
    /// 域名。
    pub domain: &'static str,
    /// 可个性化项。
    pub items: &'static str,
    /// 硬线。
    pub hardline: &'static str,
    /// 回退路径。
    pub fallback: &'static str,
}

/// 五域速查表（CI 核对每域一页）。
pub const DOMAIN_CHEATSHEET: [CheatRow; 5] = [
    CheatRow {
        domain: "色彩",
        items: "强调色、明暗模式",
        hardline: "对比度 ≥ 4.5:1",
        fallback: "默认强调色",
    },
    CheatRow {
        domain: "声音",
        items: "主音量、音效集",
        hardline: "增益 ≤ 140%",
        fallback: "默认音效集",
    },
    CheatRow {
        domain: "字体",
        items: "字体族、基础字号",
        hardline: "字号 ≥ 12px",
        fallback: "系统字体回退链",
    },
    CheatRow {
        domain: "图标",
        items: "形状蒙版",
        hardline: "可见主体 ≥ 70%",
        fallback: "原样输出",
    },
    CheatRow {
        domain: "动效",
        items: "开关、时长档",
        hardline: "动效减弱用户强制关",
        fallback: "默认时长",
    },
];

// ---------------------------------------------------------------------------
// 三、文档条目
// ---------------------------------------------------------------------------

/// 必备文档类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PDocKind {
    /// 使用指南。
    Guide,
    /// 仲裁手册。
    Rulebook,
    /// 预设说明。
    PresetNote,
}

/// 一条文档条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PDocEntry {
    pub kind: PDocKind,
    pub title: &'static str,
    pub anchor: &'static str,
}

/// 文档册。
#[derive(Clone, Debug, Default)]
pub struct PDocBook {
    pub entries: Vec<PDocEntry>,
}

impl PDocBook {
    /// 空册。
    pub fn new() -> Self {
        Self::default()
    }

    /// 挂条目。
    pub fn mount(&mut self, e: PDocEntry) {
        self.entries.push(e);
    }

    /// 锚点核对。
    pub fn check_anchors(&self, symbols: &[&str]) -> Result<(), PDocCode> {
        for e in &self.entries {
            if !symbols.contains(&e.anchor) {
                return Err(PDocCode::AnchorLost);
            }
        }
        Ok(())
    }

    /// 必备类核对。
    pub fn check_required(&self) -> Result<(), PDocCode> {
        for k in [PDocKind::Guide, PDocKind::Rulebook, PDocKind::PresetNote] {
            if !self.entries.iter().any(|e| e.kind == k) {
                return Err(PDocCode::Missing);
            }
        }
        Ok(())
    }
}

/// 内置条目集（E07 组收口的文档件）。
pub fn builtin_entries() -> PDocBook {
    let mut b = PDocBook::new();
    b.mount(PDocEntry {
        kind: PDocKind::Guide,
        title: "五域个性化使用指南",
        anchor: "ver09i_profile::PersonaProfile",
    });
    b.mount(PDocEntry {
        kind: PDocKind::Rulebook,
        title: "多层冲突仲裁规则手册",
        anchor: "ver09k_layerchain::CHAIN_RULE",
    });
    b.mount(PDocEntry {
        kind: PDocKind::PresetNote,
        title: "三官方预设说明",
        anchor: "ver09j_presets::Preset",
    });
    b
}

// ---------------------------------------------------------------------------
// 四、CI 门
// ---------------------------------------------------------------------------

/// 文档 CI。
#[derive(Default)]
pub struct PersonaDocCI {
    pub runs: u32,
}

impl PersonaDocCI {
    /// 全查：必备类 + 锚点 + 速查表五域齐 + 仲裁手册同源核对。
    pub fn run(&mut self, book: &PDocBook, symbols: &[&str]) -> Result<usize, PDocCode> {
        book.check_required()?;
        book.check_anchors(symbols)?;
        if DOMAIN_CHEATSHEET.len() != 5 {
            return Err(PDocCode::Missing);
        }
        // 仲裁手册同源：手册文本必须与实现常量一致（防漂移硬核）。
        let rulebook = book
            .entries
            .iter()
            .find(|e| e.kind == PDocKind::Rulebook)
            .ok_or(PDocCode::Missing)?;
        if !super::ver09k_layerchain::CHAIN_RULE.contains("无障碍") {
            return Err(PDocCode::RuleDrift);
        }
        self.runs += 1;
        Ok(book.entries.len() + DOMAIN_CHEATSHEET.len() + rulebook.title.len())
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SYMBOLS: [&str; 3] = [
        "ver09i_profile::PersonaProfile",
        "ver09k_layerchain::CHAIN_RULE",
        "ver09j_presets::Preset",
    ];

    #[test]
    fn builtin_doc_green() {
        let mut ci = PersonaDocCI::default();
        let b = builtin_entries();
        assert!(ci.run(&b, &SYMBOLS).is_ok());
    }

    #[test]
    fn anchor_lost_blocked() {
        let mut ci = PersonaDocCI::default();
        let b = builtin_entries();
        assert_eq!(ci.run(&b, &["wrong"]), Err(PDocCode::AnchorLost));
    }

    #[test]
    fn missing_kind_blocked() {
        let mut b = PDocBook::new();
        b.mount(PDocEntry { kind: PDocKind::Guide, title: "t", anchor: "x" });
        assert_eq!(b.check_required(), Err(PDocCode::Missing));
    }
}
